//! A one-off copy of a Local LLM PC Companion's history (its SQLite file) into
//! PostgreSQL (Phase 1, task 2): `companion-backend import-sqlite <file>`.
//!
//! Table by table, column by column: whatever columns both sides have are
//! copied, converted to PostgreSQL's types (0/1 becomes a boolean), so a file
//! from an older Companion that lacks newer columns imports with their
//! defaults. Rows are read in their original order, so conversations and
//! journals keep their order. Rows already present are skipped, so running it
//! twice copies nothing twice. It is one transaction: all or nothing.
// ponytail: imported records have no owner yet; they get the importing person
// as owner when users arrive (task 5).

use sqlx::{AssertSqlSafe, Row};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// The tables, in an order that keeps messages before their journals.
const TABLES: [&str; 18] = [
    "conversations",
    "messages",
    "message_activities",
    "tool_executions",
    "artifacts",
    "attachments",
    "settings",
    "workspaces",
    "search_runs",
    "memory_entries",
    "knowledge_chunks",
    "generation_metrics",
    "context_summaries",
    "session_task_context",
    "runtime_calibrations",
    "model_requests",
    "model_template_caps",
    "runtime_fit_decisions",
];

#[derive(Debug, Default, PartialEq)]
pub struct ImportReport {
    /// Rows copied per table.
    pub copied: Vec<(String, u64)>,
    /// Rows already present and left alone.
    pub skipped: u64,
}

enum Value {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
}

fn read_value(row: &rusqlite::Row, index: usize) -> rusqlite::Result<Value> {
    use rusqlite::types::ValueRef;
    Ok(match row.get_ref(index)? {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(n) => Value::Integer(n),
        ValueRef::Real(x) => Value::Real(x),
        ValueRef::Text(t) => Value::Text(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Value::Text(String::from_utf8_lossy(b).into_owned()),
    })
}

/// Bind one SQLite value as the PostgreSQL column's type.
fn bind<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    value: Value,
    data_type: &str,
) -> Result<sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>, String> {
    Ok(match (data_type, value) {
        ("boolean", Value::Null) => query.bind(None::<bool>),
        ("boolean", Value::Integer(n)) => query.bind(n != 0),
        ("boolean", Value::Text(t)) => query.bind(matches!(t.as_str(), "1" | "true")),
        ("bigint", Value::Null) => query.bind(None::<i64>),
        ("bigint", Value::Integer(n)) => query.bind(n),
        ("bigint", Value::Real(x)) => query.bind(x as i64),
        ("bigint", Value::Text(t)) => query.bind(t.parse::<i64>().map_err(|_| format!("{t:?} is not a whole number"))?),
        ("double precision", Value::Null) => query.bind(None::<f64>),
        ("double precision", Value::Integer(n)) => query.bind(n as f64),
        ("double precision", Value::Real(x)) => query.bind(x),
        ("double precision", Value::Text(t)) => query.bind(t.parse::<f64>().map_err(|_| format!("{t:?} is not a number"))?),
        (_, Value::Null) => query.bind(None::<String>),
        (_, Value::Integer(n)) => query.bind(n.to_string()),
        (_, Value::Real(x)) => query.bind(x.to_string()),
        (_, Value::Text(t)) => query.bind(t),
    })
}

/// Copy the history in the SQLite file at `path` into the database `pool` points at.
pub async fn import_sqlite(path: &Path, pool: &sqlx::PgPool) -> Result<ImportReport, String> {
    let source = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut tx = pool.begin().await.map_err(|e| format!("cannot start the import: {e}"))?;
    let mut report = ImportReport::default();
    let mut new_messages: HashSet<String> = HashSet::new();
    for table in TABLES {
        // The source's columns; a table this file never had is skipped.
        let source_columns: Vec<String> = {
            let mut statement = source.prepare(&format!("PRAGMA table_info({table})")).map_err(|e| e.to_string())?;
            let names = statement.query_map([], |row| row.get::<_, String>(1)).map_err(|e| e.to_string())?;
            names.collect::<Result<_, _>>().map_err(|e| e.to_string())?
        };
        if source_columns.is_empty() {
            continue;
        }
        // The destination's columns and types, leaving out generated order numbers.
        let destination: HashMap<String, String> = sqlx::query(
            "SELECT column_name::TEXT AS name, data_type::TEXT AS type FROM information_schema.columns
             WHERE table_schema = current_schema() AND table_name = $1 AND is_identity = 'NO'",
        )
        .bind(table)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .map(|row| (row.get::<String, _>("name"), row.get::<String, _>("type")))
        .collect();
        let columns: Vec<&String> = source_columns.iter().filter(|name| destination.contains_key(*name)).collect();
        let column_list = columns.iter().map(|name| format!("\"{name}\"")).collect::<Vec<_>>().join(",");
        let placeholders = (1..=columns.len()).map(|n| format!("${n}")).collect::<Vec<_>>().join(",");
        // Journals have no id of their own: they come with the messages this import adds.
        let conflict = if table == "message_activities" { "" } else { " ON CONFLICT DO NOTHING" };
        let returning = if table == "messages" { " RETURNING id" } else { "" };
        let insert = format!("INSERT INTO {table} ({column_list}) VALUES ({placeholders}){conflict}{returning}");
        let select = format!("SELECT {} FROM {table} ORDER BY rowid", columns.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(","));
        let rows: Vec<Vec<Value>> = {
            let mut statement = source.prepare(&select).map_err(|e| format!("{table}: {e}"))?;
            let mapped = statement
                .query_map([], |row| (0..columns.len()).map(|index| read_value(row, index)).collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|e| format!("{table}: {e}"))?;
            mapped.collect::<Result<_, _>>().map_err(|e| format!("{table}: {e}"))?
        };
        let message_id_at = columns.iter().position(|name| name.as_str() == "message_id");
        let mut copied = 0u64;
        for values in rows {
            if table == "message_activities" {
                let belongs = match message_id_at.map(|at| &values[at]) {
                    Some(Value::Text(id)) => new_messages.contains(id),
                    _ => false,
                };
                if !belongs {
                    report.skipped += 1;
                    continue;
                }
            }
            let mut query = sqlx::query(AssertSqlSafe(insert.clone()));
            for (value, name) in values.into_iter().zip(&columns) {
                query = bind(query, value, &destination[*name]).map_err(|e| format!("{table}.{name}: {e}"))?;
            }
            if table == "messages" {
                match query.fetch_optional(&mut *tx).await.map_err(|e| format!("{table}: {e}"))? {
                    Some(row) => {
                        new_messages.insert(row.get::<String, _>("id"));
                        copied += 1;
                    }
                    None => report.skipped += 1,
                }
            } else {
                let done = query.execute(&mut *tx).await.map_err(|e| format!("{table}: {e}"))?;
                if done.rows_affected() > 0 {
                    copied += 1;
                } else {
                    report.skipped += 1;
                }
            }
        }
        report.copied.push((table.to_string(), copied));
    }
    tx.commit().await.map_err(|e| format!("the import was not saved: {e}"))?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An old Companion file: an early schema without the later columns.
    fn old_companion_file() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("companion-import-{}.db", uuid::Uuid::new_v4().simple()));
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE conversations(id TEXT PRIMARY KEY, title TEXT NOT NULL, model_id TEXT NOT NULL, created_at TEXT NOT NULL,
                 reasoning_default INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE messages(id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL, created_at TEXT NOT NULL);
             CREATE TABLE message_activities(message_id TEXT NOT NULL, event_json TEXT NOT NULL);
             CREATE TABLE tool_executions(id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL, tool TEXT NOT NULL, args TEXT NOT NULL,
                 result TEXT NOT NULL, approved INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL);
             CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO conversations VALUES('c1','Budget','m','2026-09-01T00:00:00Z',1);
             INSERT INTO messages VALUES('b','c1','user','second written first','same time');
             INSERT INTO messages VALUES('a','c1','assistant','then this','same time');
             INSERT INTO tool_executions VALUES('t1','c1','read_file','{}','ok',1,'2026-09-01T00:00:01Z');
             INSERT INTO settings VALUES('app_settings','{}');",
        )
        .unwrap();
        let event = crate::agent::AgentEvent::activity("status", crate::agent::AgentState::Completed, "done".into(), 1);
        db.execute("INSERT INTO message_activities VALUES('a', ?1)", [serde_json::to_string(&event).unwrap()]).unwrap();
        path
    }

    #[tokio::test]
    async fn an_old_companion_file_imports_in_order_and_only_once() {
        let storage = crate::storage::testing::storage();
        let file = old_companion_file();
        let report = import_sqlite(&file, storage.pool()).await.unwrap();
        let copied: HashMap<String, u64> = report.copied.into_iter().collect();
        assert_eq!(copied["conversations"], 1);
        assert_eq!(copied["messages"], 2);
        assert_eq!(copied["message_activities"], 1);
        assert_eq!(copied["tool_executions"], 1);

        let conversation = storage.get_conversation("c1").await.unwrap().unwrap();
        assert!(conversation.reasoning_default, "0/1 became a boolean");
        assert_eq!(conversation.mode, "chat", "a column the old file lacks takes its default");
        let messages = storage.messages_for("c1").await.unwrap();
        assert_eq!(messages.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["b", "a"], "the file's order, not alphabetical");
        assert!(storage.tool_executions_for("c1", 10).await.unwrap()[0].approved);

        let again = import_sqlite(&file, storage.pool()).await.unwrap();
        assert!(again.copied.iter().all(|(_, n)| *n == 0), "{again:?}");
        assert_eq!(storage.messages_for("c1").await.unwrap().len(), 2);
        assert_eq!(storage.message_activities("a").await.unwrap().len(), 1, "the journal came with its message, once");
        let _ = std::fs::remove_file(file);
    }
}
