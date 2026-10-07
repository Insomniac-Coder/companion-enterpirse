//! Persistence (§20, §70) in PostgreSQL: conversations, messages, artifacts,
//! settings and records. The schema lives in `backend/migrations`.

use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::Row;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub model_id: String,
    pub created_at: String,
    /// Stage 14 session mode: "chat" | "code".
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Stage 14: linked workspace id (code sessions) or "" .
    #[serde(default)]
    pub workspace: String,
    /// Stage 11: per-conversation capability defaults (§115).
    #[serde(default)]
    pub reasoning_default: bool,
    #[serde(default)]
    pub search_default: bool,
    /// Stage 17: model that last prepared/generated this thread (§170).
    /// The composer blocks when it differs from the loaded model.
    #[serde(default)]
    pub last_model: String,
    /// Stage 27: scheduling priority — "background" | "normal" | "high".
    #[serde(default = "default_priority")]
    pub priority: String,
    /// Stage 27: optional related session id (§146 metadata link, not shared ctx).
    #[serde(default)]
    pub related_to: String,
    /// The person whose conversation it is.
    #[serde(default)]
    pub user_id: String,
}

fn default_priority() -> String {
    "normal".into()
}

fn default_mode() -> String {
    "chat".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub role: String, // user | assistant | tool
    pub content: String,
    pub created_at: String,
}

/// Stage 6: file attached to a conversation (§67). The bytes live on disk
/// under data/attachments/<conv>/; only extracted text is stored in-DB.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub conversation_id: String,
    pub filename: String,
    pub mime: String,
    pub size_bytes: u64,
    pub text_excerpt: String,
    /// Stage 19: "text" | "image" (§76 status + §73 fallback routing).
    #[serde(default = "default_attach_kind")]
    pub kind: String,
    /// Stage 28: processing state — "ready" | "partial" | "processing" | "unsupported".
    #[serde(default = "default_attach_status")]
    pub status: String,
    pub created_at: String,
}

fn default_attach_status() -> String {
    "ready".into()
}

fn default_attach_kind() -> String {
    "text".into()
}

/// Stage 14: named project boundary (§27, §67).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub path: String,
    pub build_system: String,
    pub created_at: String,
    /// The person whose project it is.
    #[serde(default)]
    pub user_id: String,
}

/// Stage 11: record of an explicit web-search request (§121).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRun {
    pub id: String,
    pub conversation_id: String,
    pub query: String,
    pub provider: String,
    pub result_count: usize,
    pub created_at: String,
}

/// Stage 20: generated file record (§38). Bytes live on disk; the row is
/// what the UI renders as an artifact card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRow {
    pub id: String,
    pub conversation_id: String,
    pub filename: String,
    pub path: String,
    pub mime: String,
    pub size_bytes: u64,
    pub created_at: String,
}

/// Stage 26: scoped memory entry (§§82–85). Never bulk-injected into
/// context; the context manager resolves only explicit references.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    /// empty = global; otherwise conversation id or workspace id.
    pub scope_id: String,
    /// "session" | "conversation" | "workspace" | "global"
    pub scope: String,
    pub content: String,
    pub source: String,
    pub created_at: String,
    pub last_used: String,
    /// The person who saved it.
    #[serde(default)]
    pub user_id: String,
}

#[derive(Debug, Default)]
pub struct MemoryContext {
    pub text: String,
    pub entries: usize,
}

#[derive(Debug, Clone)]
pub struct TaskContext {
    pub conversation_id: String,
    pub workspace: String,
    pub task: String,
    pub run_id: String,
    /// active | planned | interrupted | completed
    pub status: String,
}

/// Stage 35 local knowledge chunk (§39): parsed text kept locally for
/// keyword retrieval. Vector embeddings are staged; scoring is explicit
/// term overlap so results are explainable, never magic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeChunk {
    pub id: String,
    pub workspace_id: String,
    pub path: String,
    pub chunk_idx: i64,
    pub text: String,
}
/// Stage 33 per-message generation metrics (§§32–34): kept separate from
/// message content so telemetry aggregates without duplicating threads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationMetric {
    pub message_id: String,
    pub conversation_id: String,
    pub model_id: String,
    pub prompt_tokens: u32,
    pub generated_tokens: u32,
    pub gen_ms: u64,
    pub ttft_ms: u64,
    pub gen_tps: f64,
    #[serde(default)]
    pub timing: Option<crate::inference::OutputTiming>,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecution {
    pub id: String,
    pub conversation_id: String,
    pub tool: String,
    pub args: String,
    pub result: String,
    pub approved: bool,
    /// How the action was allowed: by the permission mode, a session grant,
    /// the user's approval, or chat's read-only set (or why it was refused).
    /// Empty in records written before this was kept.
    #[serde(default)]
    pub approval: String,
    pub created_at: String,
}

/// Every read and write of the application's records. One PostgreSQL pool,
/// shared by all requests: there is no lock to wait on, so one slow request no
/// longer holds up every other (the SQLite version had one connection behind
/// one lock, 93 places waiting on it).
#[derive(Clone)]
pub struct Storage {
    pool: PgPool,
}

pub type DbResult<T> = Result<T, sqlx::Error>;

/// The schema, as numbered files in `backend/migrations`, built into the program.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

fn encode_error(error: serde_json::Error) -> sqlx::Error {
    sqlx::Error::Encode(Box::new(error))
}

fn conversation(r: &PgRow) -> DbResult<Conversation> {
    Ok(Conversation {
        id: r.try_get("id")?,
        title: r.try_get("title")?,
        model_id: r.try_get("model_id")?,
        created_at: r.try_get("created_at")?,
        mode: r.try_get("mode")?,
        workspace: r.try_get("workspace")?,
        reasoning_default: r.try_get("reasoning_default")?,
        search_default: r.try_get("search_default")?,
        last_model: r.try_get("last_model")?,
        priority: r.try_get("priority")?,
        related_to: r.try_get("related_to")?,
        user_id: r.try_get("user_id")?,
    })
}

fn message(r: &PgRow) -> DbResult<Message> {
    Ok(Message {
        id: r.try_get("id")?,
        conversation_id: r.try_get("conversation_id")?,
        role: r.try_get("role")?,
        content: r.try_get("content")?,
        created_at: r.try_get("created_at")?,
    })
}

fn attachment(r: &PgRow) -> DbResult<Attachment> {
    Ok(Attachment {
        id: r.try_get("id")?,
        conversation_id: r.try_get("conversation_id")?,
        filename: r.try_get("filename")?,
        mime: r.try_get("mime")?,
        size_bytes: r.try_get::<i64, _>("size_bytes")? as u64,
        text_excerpt: r.try_get("text_excerpt")?,
        kind: r.try_get("kind")?,
        status: r.try_get("status")?,
        created_at: r.try_get("created_at")?,
    })
}

fn artifact(r: &PgRow) -> DbResult<ArtifactRow> {
    Ok(ArtifactRow {
        id: r.try_get("id")?,
        conversation_id: r.try_get("conversation_id")?,
        filename: r.try_get("filename")?,
        path: r.try_get("path")?,
        mime: r.try_get("mime")?,
        size_bytes: r.try_get::<i64, _>("size_bytes")? as u64,
        created_at: r.try_get("created_at")?,
    })
}

fn workspace(r: &PgRow) -> DbResult<Workspace> {
    Ok(Workspace {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        path: r.try_get("path")?,
        build_system: r.try_get("build_system")?,
        created_at: r.try_get("created_at")?,
        user_id: r.try_get("user_id")?,
    })
}

fn memory(r: &PgRow) -> DbResult<MemoryEntry> {
    Ok(MemoryEntry {
        id: r.try_get("id")?,
        scope_id: r.try_get("scope_id")?,
        scope: r.try_get("scope")?,
        content: r.try_get("content")?,
        source: r.try_get("source")?,
        created_at: r.try_get("created_at")?,
        last_used: r.try_get("last_used")?,
        user_id: r.try_get("user_id")?,
    })
}

fn tool_execution(r: &PgRow) -> DbResult<ToolExecution> {
    Ok(ToolExecution {
        id: r.try_get("id")?,
        conversation_id: r.try_get("conversation_id")?,
        tool: r.try_get("tool")?,
        args: r.try_get("args")?,
        result: r.try_get("result")?,
        approved: r.try_get("approved")?,
        approval: r.try_get("approval")?,
        created_at: r.try_get("created_at")?,
    })
}

fn model_request(r: &PgRow) -> DbResult<ModelRequestRecord> {
    Ok(ModelRequestRecord {
        id: r.try_get("id")?,
        conversation_id: r.try_get("conversation_id")?,
        owner_id: r.try_get("owner_id")?,
        seq: r.try_get::<i64, _>("seq")? as u32,
        kind: r.try_get("kind")?,
        request_json: r.try_get("request_json")?,
        raw_output: r.try_get("raw_output")?,
        finish_reason: r.try_get("finish_reason")?,
        outcome: r.try_get("outcome")?,
        failure: r.try_get("failure")?,
        prompt_tokens: r.try_get::<i64, _>("prompt_tokens")? as u32,
        cached_tokens: r.try_get::<i64, _>("cached_tokens")? as u32,
        generated_tokens: r.try_get::<i64, _>("generated_tokens")? as u32,
        created_at: r.try_get("created_at")?,
    })
}

fn metric(r: &PgRow) -> DbResult<GenerationMetric> {
    Ok(GenerationMetric {
        message_id: r.try_get("message_id")?,
        conversation_id: r.try_get("conversation_id")?,
        model_id: r.try_get("model_id")?,
        prompt_tokens: r.try_get::<i64, _>("prompt_tokens")? as u32,
        generated_tokens: r.try_get::<i64, _>("generated_tokens")? as u32,
        gen_ms: r.try_get::<i64, _>("gen_ms")? as u64,
        ttft_ms: r.try_get::<i64, _>("ttft_ms")? as u64,
        gen_tps: r.try_get("gen_tps")?,
        created_at: r.try_get("created_at")?,
        timing: r
            .try_get::<Option<String>, _>("timing_json")?
            .map(|json| serde_json::from_str(&json).map_err(|error| sqlx::Error::Decode(Box::new(error))))
            .transpose()?,
    })
}

impl Storage {
    /// The connection pool, for work that is not a record of its own (the import).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Connect to the database at `url`, bring its schema up to date, and mark
    /// work that a previous run left unfinished as interrupted.
    pub async fn open(url: &str) -> DbResult<Self> {
        let pool = PgPoolOptions::new().max_connections(16).connect(url).await?;
        MIGRATOR.run(&pool).await.map_err(|error| sqlx::Error::Migrate(Box::new(error)))?;
        let storage = Self { pool };
        storage.recover_interrupted_activity().await?;
        Ok(storage)
    }

    pub async fn load_settings(&self) -> DbResult<Option<crate::settings::AppSettings>> {
        let stored: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key='app_settings'")
            .fetch_optional(&self.pool)
            .await?;
        stored
            .map(|value| serde_json::from_str(&value).map_err(|error| sqlx::Error::Decode(Box::new(error))))
            .transpose()
    }

    pub async fn save_settings(&self, settings: &crate::settings::AppSettings) -> DbResult<()> {
        let value = serde_json::to_string(settings).map_err(encode_error)?;
        sqlx::query("INSERT INTO settings(key,value) VALUES('app_settings',$1) ON CONFLICT(key) DO UPDATE SET value=excluded.value")
            .bind(value)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_conversation(&self, c: &Conversation) -> DbResult<()> {
        sqlx::query("INSERT INTO conversations(id,title,model_id,created_at,mode,workspace,reasoning_default,search_default,last_model,priority,related_to,user_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
            .bind(&c.id).bind(&c.title).bind(&c.model_id).bind(&c.created_at).bind(&c.mode).bind(&c.workspace)
            .bind(c.reasoning_default).bind(c.search_default).bind(&c.last_model).bind(&c.priority).bind(&c.related_to).bind(&c.user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Stage 14: update mutable session fields (title, model, mode, workspace, defaults).
    /// Save the fields `c` changes from `before`, and only those: two edits
    /// of different fields at once both stay, where writing the whole row let
    /// the second undo the first.
    pub async fn update_conversation(&self, before: &Conversation, c: &Conversation) -> DbResult<bool> {
        fn changed<'a, T: PartialEq>(old: &T, new: &'a T) -> Option<&'a T> {
            (old != new).then_some(new)
        }
        let done = sqlx::query(
            "UPDATE conversations SET title=COALESCE($1,title),model_id=COALESCE($2,model_id),mode=COALESCE($3,mode),
             workspace=COALESCE($4,workspace),reasoning_default=COALESCE($5,reasoning_default),search_default=COALESCE($6,search_default),
             last_model=COALESCE($7,last_model),priority=COALESCE($8,priority),related_to=COALESCE($9,related_to) WHERE id=$10",
        )
        .bind(changed(&before.title, &c.title))
        .bind(changed(&before.model_id, &c.model_id))
        .bind(changed(&before.mode, &c.mode))
        .bind(changed(&before.workspace, &c.workspace))
        .bind(changed(&before.reasoning_default, &c.reasoning_default))
        .bind(changed(&before.search_default, &c.search_default))
        .bind(changed(&before.last_model, &c.last_model))
        .bind(changed(&before.priority, &c.priority))
        .bind(changed(&before.related_to, &c.related_to))
        .bind(&c.id)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Stage 17: record which model prepared/generated a thread (§170).
    pub async fn set_last_model(&self, conv: &str, model: &str) -> DbResult<()> {
        sqlx::query("UPDATE conversations SET last_model=$1 WHERE id=$2")
            .bind(model)
            .bind(conv)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// One person's conversations, newest first.
    pub async fn list_conversations(&self, user: &str) -> DbResult<Vec<Conversation>> {
        sqlx::query("SELECT id,title,model_id,created_at,mode,workspace,reasoning_default,search_default,last_model,priority,related_to,user_id FROM conversations WHERE user_id=$1 ORDER BY created_at DESC")
            .bind(user)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(conversation)
            .collect()
    }

    pub async fn get_conversation(&self, id: &str) -> DbResult<Option<Conversation>> {
        sqlx::query("SELECT id,title,model_id,created_at,mode,workspace,reasoning_default,search_default,last_model,priority,related_to,user_id FROM conversations WHERE id=$1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .map(conversation)
            .transpose()
    }

    /// Removes the chat rows of a conversation, all or nothing. Tool history
    /// rows go with it; artifact files stay on disk (artifact clean-up is its
    /// own job).
    pub async fn delete_conversation(&self, id: &str) -> DbResult<bool> {
        let mut tx = self.pool.begin().await?;
        for statement in [
            "DELETE FROM session_task_context WHERE conversation_id=$1",
            "DELETE FROM messages WHERE conversation_id=$1",
            "DELETE FROM tool_executions WHERE conversation_id=$1",
            "DELETE FROM attachments WHERE conversation_id=$1",
        ] {
            sqlx::query(statement).bind(id).execute(&mut *tx).await?;
        }
        let done = sqlx::query("DELETE FROM conversations WHERE id=$1").bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(done.rows_affected() > 0)
    }

    pub async fn add_message(&self, m: &Message) -> DbResult<()> {
        sqlx::query("INSERT INTO messages(id,conversation_id,role,content,created_at) VALUES($1,$2,$3,$4,$5)")
            .bind(&m.id).bind(&m.conversation_id).bind(&m.role).bind(&m.content).bind(&m.created_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn message_count(&self, conv: &str) -> DbResult<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE conversation_id=$1")
            .bind(conv)
            .fetch_one(&self.pool)
            .await?;
        Ok(count as u64)
    }

    pub async fn messages_for(&self, conv: &str) -> DbResult<Vec<Message>> {
        sqlx::query(MESSAGES_OF).bind(conv).fetch_all(&self.pool).await?.iter().map(message).collect()
    }

    /// A conversation's messages and their journals, read from one snapshot: a reply that finishes
    /// meanwhile is seen whole or not at all, never its new journal beside its old text.
    pub async fn messages_with_activities(&self, conv: &str) -> DbResult<(Vec<Message>, ActivitiesByMessage)> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY").execute(&mut *tx).await?;
        let messages = sqlx::query(MESSAGES_OF).bind(conv).fetch_all(&mut *tx).await?.iter().map(message).collect::<DbResult<Vec<_>>>()?;
        let journals = group_activities(sqlx::query(ACTIVITIES_OF).bind(conv).fetch_all(&mut *tx).await?)?;
        tx.commit().await?;
        Ok((messages, journals))
    }

    /// A derived inference view. Canonical messages, IDs and timestamps are
    /// never rewritten by compaction, so metrics and forks stay valid.
    pub async fn context_messages_for(&self, conv: &str) -> DbResult<Vec<Message>> {
        let history = self.messages_for(conv).await?;
        let summary = sqlx::query("SELECT source_ids,content,created_at FROM context_summaries WHERE conversation_id=$1")
            .bind(conv)
            .fetch_optional(&self.pool)
            .await?;
        let Some(summary) = summary else {
            return Ok(history);
        };
        let encoded_ids: String = summary.try_get("source_ids")?;
        let content: String = summary.try_get("content")?;
        let created_at: String = summary.try_get("created_at")?;
        let ids: Vec<String> = serde_json::from_str(&encoded_ids).unwrap_or_default();
        // An edit/truncate or old database must never attach a summary to a
        // different prefix. Fall back to canonical history if it no longer fits.
        if ids.is_empty()
            || history.len() < ids.len()
            || !history.iter().zip(&ids).all(|(message, id)| message.id == *id)
        {
            return Ok(history);
        }
        let mut view = vec![Message {
            id: format!("context-summary:{conv}"),
            conversation_id: conv.into(),
            role: "assistant".into(),
            content,
            created_at,
        }];
        view.extend(history.into_iter().skip(ids.len()));
        Ok(view)
    }

    pub async fn save_context_summary(&self, conv: &str, source_ids: &[String], content: &str) -> DbResult<()> {
        sqlx::query("INSERT INTO context_summaries(conversation_id,source_ids,content,created_at) VALUES($1,$2,$3,$4)
             ON CONFLICT(conversation_id) DO UPDATE SET source_ids=excluded.source_ids,content=excluded.content,created_at=excluded.created_at")
            .bind(conv)
            .bind(serde_json::to_string(source_ids).unwrap_or_default())
            .bind(content)
            .bind(chrono::Utc::now().to_rfc3339())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn record_message_activity(&self, mid: &str, event: &crate::agent::AgentEvent) -> DbResult<()> {
        sqlx::query("INSERT INTO message_activities(message_id,event_json) VALUES($1,$2)")
            .bind(mid)
            .bind(serde_json::to_string(event).unwrap_or_default())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// A reply's last journal event and its final text, together: a reader
    /// never sees the run ended with the reply still saying it is starting.
    pub async fn finish_message(&self, conv: &str, mid: &str, event: &crate::agent::AgentEvent) -> DbResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO message_activities(message_id,event_json) VALUES($1,$2)")
            .bind(mid)
            .bind(serde_json::to_string(event).unwrap_or_default())
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM context_summaries WHERE conversation_id=$1").bind(conv).execute(&mut *tx).await?;
        sqlx::query("UPDATE messages SET content=$1 WHERE conversation_id=$2 AND id=$3")
            .bind(&event.message)
            .bind(conv)
            .bind(mid)
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }

    pub async fn message_activities(&self, mid: &str) -> DbResult<Vec<crate::agent::AgentEvent>> {
        let rows: Vec<String> = sqlx::query_scalar("SELECT event_json FROM message_activities WHERE message_id=$1 ORDER BY seq")
            .bind(mid)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().filter_map(|json| serde_json::from_str(json).ok()).collect())
    }

    pub async fn save_task_context(&self, context: &TaskContext) -> DbResult<()> {
        sqlx::query("INSERT INTO session_task_context(conversation_id,workspace,task,run_id,status) VALUES($1,$2,$3,$4,$5)
            ON CONFLICT(conversation_id) DO UPDATE SET workspace=excluded.workspace,task=excluded.task,run_id=excluded.run_id,status=excluded.status")
            .bind(&context.conversation_id).bind(&context.workspace).bind(&context.task).bind(&context.run_id).bind(&context.status)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn task_context(&self, conversation_id: &str) -> DbResult<Option<TaskContext>> {
        let row = sqlx::query("SELECT conversation_id,workspace,task,run_id,status FROM session_task_context WHERE conversation_id=$1")
            .bind(conversation_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| {
            Ok(TaskContext {
                conversation_id: row.try_get("conversation_id")?,
                workspace: row.try_get("workspace")?,
                task: row.try_get("task")?,
                run_id: row.try_get("run_id")?,
                status: row.try_get("status")?,
            })
        })
        .transpose()
    }

    pub async fn finish_task_context(&self, conversation_id: &str, run_id: &str, status: &str) -> DbResult<()> {
        sqlx::query("UPDATE session_task_context SET status=$1 WHERE conversation_id=$2 AND run_id=$3")
            .bind(status)
            .bind(conversation_id)
            .bind(run_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn clear_task_context(&self, conversation_id: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM session_task_context WHERE conversation_id=$1")
            .bind(conversation_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Reopening a database does not resume an old process. Mark unfinished
    /// journals honestly instead of leaving historical actions spinning forever.
    // ponytail: one server owns every run in Phase 1; with several servers
    // (Phase 8) each run needs an owner and only that server's runs are marked.
    pub(crate) async fn recover_interrupted_activity(&self) -> DbResult<()> {
        sqlx::query("UPDATE session_task_context SET status='interrupted' WHERE status='active'")
            .execute(&self.pool)
            .await?;
        let records = sqlx::query(
            "SELECT m.id,m.conversation_id,m.content,a.event_json FROM messages m
             JOIN message_activities a ON a.seq=(SELECT MAX(seq) FROM message_activities WHERE message_id=m.id)",
        )
        .fetch_all(&self.pool)
        .await?;
        for record in records {
            let id: String = record.try_get("id")?;
            let conversation: String = record.try_get("conversation_id")?;
            let content: String = record.try_get("content")?;
            let json: String = record.try_get("event_json")?;
            let Ok(last) = serde_json::from_str::<crate::agent::AgentEvent>(&json) else {
                continue;
            };
            if matches!(
                last.state,
                crate::agent::AgentState::Completed | crate::agent::AgentState::Failed | crate::agent::AgentState::Cancelled
            ) {
                continue;
            }
            let message = "Interrupted when the runtime closed. Recorded actions and existing files were preserved. Review the partial work before starting again.";
            self.record_message_activity(
                &id,
                &crate::agent::AgentEvent::activity("status", crate::agent::AgentState::Cancelled, message.into(), last.iteration),
            )
            .await?;
            if content == "Work is starting…" {
                self.update_message_content(&conversation, &id, message).await?;
            }
        }
        Ok(())
    }

    pub async fn get_message(&self, conv: &str, mid: &str) -> DbResult<Option<Message>> {
        sqlx::query("SELECT id,conversation_id,role,content,created_at FROM messages WHERE conversation_id=$1 AND id=$2")
            .bind(conv)
            .bind(mid)
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .map(message)
            .transpose()
    }

    pub async fn update_message_content(&self, conv: &str, mid: &str, content: &str) -> DbResult<bool> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM context_summaries WHERE conversation_id=$1").bind(conv).execute(&mut *tx).await?;
        let done = sqlx::query("UPDATE messages SET content=$1 WHERE conversation_id=$2 AND id=$3")
            .bind(content)
            .bind(conv)
            .bind(mid)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(done.rows_affected() > 0)
    }

    /// Delete every message inserted after `mid`. Returns the count.
    /// Used by edit-with-truncate: editing an old turn restarts the thread there.
    pub async fn delete_messages_after(&self, conv: &str, mid: &str) -> DbResult<usize> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM session_task_context WHERE conversation_id=$1").bind(conv).execute(&mut *tx).await?;
        let done = sqlx::query("DELETE FROM messages WHERE conversation_id=$1 AND seq > (SELECT seq FROM messages WHERE conversation_id=$1 AND id=$2)")
            .bind(conv)
            .bind(mid)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(done.rows_affected() as usize)
    }

    /// Delete an explicit id set (compaction). Returns the count.
    pub async fn delete_messages(&self, conv: &str, ids: &[String]) -> DbResult<usize> {
        let done = sqlx::query("DELETE FROM messages WHERE conversation_id=$1 AND id = ANY($2)")
            .bind(conv)
            .bind(ids)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() as usize)
    }

    pub async fn add_attachment(&self, a: &Attachment) -> DbResult<()> {
        sqlx::query("INSERT INTO attachments(id,conversation_id,filename,mime,size_bytes,text_excerpt,kind,status,created_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(&a.id).bind(&a.conversation_id).bind(&a.filename).bind(&a.mime).bind(a.size_bytes as i64)
            .bind(&a.text_excerpt).bind(&a.kind).bind(&a.status).bind(&a.created_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn attachments_for(&self, conv: &str) -> DbResult<Vec<Attachment>> {
        sqlx::query("SELECT id,conversation_id,filename,mime,size_bytes,text_excerpt,kind,status,created_at FROM attachments WHERE conversation_id=$1 ORDER BY seq")
            .bind(conv)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(attachment)
            .collect()
    }

    pub async fn delete_attachment(&self, conv: &str, aid: &str) -> DbResult<bool> {
        let done = sqlx::query("DELETE FROM attachments WHERE conversation_id=$1 AND id=$2")
            .bind(conv)
            .bind(aid)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    /// The tool record, and its audit record (`who` asked for it) in the same transaction.
    pub async fn record_tool_execution(&self, t: &ToolExecution, who: &crate::audit::Who) -> DbResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO tool_executions(id,conversation_id,tool,args,result,approved,approval,created_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(&t.id).bind(&t.conversation_id).bind(&t.tool).bind(&t.args).bind(&t.result)
            .bind(t.approved).bind(&t.approval).bind(&t.created_at)
            .execute(&mut *tx)
            .await?;
        let record = crate::audit::Record {
            action: "tool_call",
            target: t.conversation_id.clone(),
            allowed_by: t.approval.clone(),
            outcome: if t.approval.starts_with("refused") { "refused" } else { "ran" }.into(),
            detail: serde_json::json!({ "tool": t.tool, "args": crate::audit::excerpt(&t.args) }),
            ..Default::default()
        };
        crate::audit::insert(&mut *tx, who, &record).await?;
        tx.commit().await
    }

    pub async fn tool_executions_for(&self, conv: &str, limit: usize) -> DbResult<Vec<ToolExecution>> {
        sqlx::query("SELECT id,conversation_id,tool,args,result,approved,approval,created_at FROM tool_executions WHERE conversation_id=$1 ORDER BY seq DESC LIMIT $2")
            .bind(conv)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(tool_execution)
            .collect()
    }

    // ---- Stage 20 artifacts ----

    pub async fn record_artifact(&self, a: &ArtifactRow) -> DbResult<()> {
        sqlx::query("INSERT INTO artifacts(id,conversation_id,filename,path,mime,size_bytes,created_at) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(&a.id).bind(&a.conversation_id).bind(&a.filename).bind(&a.path).bind(&a.mime)
            .bind(a.size_bytes as i64).bind(&a.created_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn artifacts_for(&self, conv: &str) -> DbResult<Vec<ArtifactRow>> {
        sqlx::query("SELECT id,conversation_id,filename,path,mime,size_bytes,created_at FROM artifacts WHERE conversation_id=$1 ORDER BY seq")
            .bind(conv)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(artifact)
            .collect()
    }

    pub async fn get_artifact(&self, aid: &str) -> DbResult<Option<ArtifactRow>> {
        sqlx::query("SELECT id,conversation_id,filename,path,mime,size_bytes,created_at FROM artifacts WHERE id=$1")
            .bind(aid)
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .map(artifact)
            .transpose()
    }

    // ---- Stage 14 workspaces ----

    pub async fn create_workspace(&self, w: &Workspace) -> DbResult<()> {
        sqlx::query("INSERT INTO workspaces(id,name,path,build_system,created_at,user_id) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(&w.id).bind(&w.name).bind(&w.path).bind(&w.build_system).bind(&w.created_at).bind(&w.user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// One person's projects.
    pub async fn list_workspaces(&self, user: &str) -> DbResult<Vec<Workspace>> {
        sqlx::query("SELECT id,name,path,build_system,created_at,user_id FROM workspaces WHERE user_id=$1 ORDER BY created_at")
            .bind(user)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(workspace)
            .collect()
    }

    pub async fn get_workspace(&self, id: &str) -> DbResult<Option<Workspace>> {
        sqlx::query("SELECT id,name,path,build_system,created_at,user_id FROM workspaces WHERE id=$1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .map(workspace)
            .transpose()
    }

    /// The chats linked to one project, newest first. Removing a project
    /// offers to take them with it; without this they would be left pointing
    /// at a project that no longer exists.
    pub async fn conversations_in_workspace(&self, workspace: &str) -> DbResult<Vec<String>> {
        sqlx::query_scalar("SELECT id FROM conversations WHERE workspace=$1 ORDER BY created_at DESC")
            .bind(workspace)
            .fetch_all(&self.pool)
            .await
    }

    pub async fn delete_workspace(&self, id: &str) -> DbResult<bool> {
        let done = sqlx::query("DELETE FROM workspaces WHERE id=$1").bind(id).execute(&self.pool).await?;
        Ok(done.rows_affected() > 0)
    }

    // ---- Stage 11 search audit ----

    /// The search record, and its audit record (`who` asked for it) in the same transaction.
    pub async fn record_search_run(&self, run: &SearchRun, who: &crate::audit::Who) -> DbResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO search_runs(id,conversation_id,query,provider,result_count,created_at) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(&run.id).bind(&run.conversation_id).bind(&run.query).bind(&run.provider)
            .bind(run.result_count as i64).bind(&run.created_at)
            .execute(&mut *tx)
            .await?;
        let record = crate::audit::Record {
            action: "web_search",
            target: run.conversation_id.clone(),
            outcome: format!("{} results", run.result_count),
            detail: serde_json::json!({ "provider": run.provider, "query": crate::audit::excerpt(&run.query) }),
            ..Default::default()
        };
        crate::audit::insert(&mut *tx, who, &record).await?;
        tx.commit().await
    }

    // ---- Stage 26 memory entries ----

    pub async fn add_memory(&self, m: &MemoryEntry) -> DbResult<()> {
        if !["session", "conversation", "workspace", "global"].contains(&m.scope.as_str()) {
            return Err(sqlx::Error::InvalidArgument("bad memory scope".into()));
        }
        sqlx::query("INSERT INTO memory_entries(id,scope_id,scope,content,source,created_at,last_used,user_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(&m.id).bind(&m.scope_id).bind(&m.scope).bind(&m.content).bind(&m.source).bind(&m.created_at).bind(&m.last_used).bind(&m.user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Memories visible to a session of `user`'s: its own conversation id, its
    /// workspace id, and their globals. Never the whole store (§82: no silent
    /// bulk dump), and never anyone else's: "global" means global to one person.
    pub async fn memories_for(&self, user: &str, conv_id: &str, workspace_id: &str) -> DbResult<Vec<MemoryEntry>> {
        sqlx::query(
            "SELECT id,scope_id,scope,content,source,created_at,last_used,user_id FROM memory_entries
             WHERE user_id=$3 AND (scope='global' OR (scope IN ('conversation','session') AND scope_id=$1) OR (scope='workspace' AND scope_id=$2))
             ORDER BY seq",
        )
        .bind(conv_id)
        .bind(workspace_id)
        .bind(user)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(memory)
        .collect()
    }

    pub async fn delete_memory(&self, id: &str) -> DbResult<bool> {
        let done = sqlx::query("DELETE FROM memory_entries WHERE id=$1").bind(id).execute(&self.pool).await?;
        Ok(done.rows_affected() > 0)
    }

    pub async fn get_memory(&self, id: &str) -> DbResult<Option<MemoryEntry>> {
        sqlx::query("SELECT id,scope_id,scope,content,source,created_at,last_used,user_id FROM memory_entries WHERE id=$1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .map(memory)
            .transpose()
    }

    pub async fn touch_memory(&self, id: &str, now: &str) -> DbResult<()> {
        sqlx::query("UPDATE memory_entries SET last_used=$1 WHERE id=$2").bind(now).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn memory_export(&self, user: &str, conv_id: &str, workspace_id: &str) -> DbResult<Vec<MemoryEntry>> {
        self.memories_for(user, conv_id, workspace_id).await
    }

    /// Bounded, deterministic context from explicitly saved memories only.
    /// The legacy session scope is a conversation alias, never a workspace.
    pub async fn memory_context(&self, user: &str, conv_id: &str, workspace_id: &str) -> DbResult<MemoryContext> {
        Ok(memory_context_from(&self.memories_for(user, conv_id, workspace_id).await?))
    }

    // ---- Stage 35 knowledge chunks ----

    /// One file's chunks replaced as one step. Two re-indexes of the same
    /// file take turns, so neither leaves its chunks beside the other's.
    pub async fn replace_knowledge_path(&self, ws: &str, path: &str, pieces: &[String]) -> DbResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1 || ':' || $2, 0))")
            .bind(ws)
            .bind(path)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM knowledge_chunks WHERE workspace_id=$1 AND path=$2").bind(ws).bind(path).execute(&mut *tx).await?;
        for (index, text) in pieces.iter().enumerate() {
            sqlx::query("INSERT INTO knowledge_chunks(id,workspace_id,path,chunk_idx,text) VALUES($1,$2,$3,$4,$5)")
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(ws)
                .bind(path)
                .bind(index as i64)
                .bind(text)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await
    }

    pub async fn knowledge_paths(&self, ws: &str) -> DbResult<Vec<(String, i64)>> {
        let rows = sqlx::query("SELECT path, COUNT(*) AS chunks FROM knowledge_chunks WHERE workspace_id=$1 GROUP BY path ORDER BY path")
            .bind(ws)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|row| Ok((row.try_get("path")?, row.try_get("chunks")?))).collect()
    }

    pub async fn clear_knowledge_path(&self, ws: &str, path: &str) -> DbResult<usize> {
        let done = sqlx::query("DELETE FROM knowledge_chunks WHERE workspace_id=$1 AND path=$2")
            .bind(ws)
            .bind(path)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() as usize)
    }

    pub async fn knowledge_for(&self, ws: &str) -> DbResult<Vec<KnowledgeChunk>> {
        let rows = sqlx::query("SELECT id,workspace_id,path,chunk_idx,text FROM knowledge_chunks WHERE workspace_id=$1 ORDER BY path,chunk_idx")
            .bind(ws)
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| {
                Ok(KnowledgeChunk {
                    id: r.try_get("id")?,
                    workspace_id: r.try_get("workspace_id")?,
                    path: r.try_get("path")?,
                    chunk_idx: r.try_get("chunk_idx")?,
                    text: r.try_get("text")?,
                })
            })
            .collect()
    }

    // ---- Calibration, template capabilities, fit decisions ----

    /// Keep a calibration. Older ones stay: they are what a later run is
    /// compared against to detect a regression.
    pub async fn save_calibration(&self, calibration: &crate::calibration::Calibration) -> DbResult<()> {
        let json = serde_json::to_string(calibration).map_err(encode_error)?;
        sqlx::query("INSERT INTO runtime_calibrations(id,model_id,model_key,created_at,calibration_json) VALUES($1,$2,$3,$4,$5)
             ON CONFLICT(id) DO UPDATE SET model_id=excluded.model_id,model_key=excluded.model_key,created_at=excluded.created_at,calibration_json=excluded.calibration_json")
            .bind(&calibration.id).bind(&calibration.model_id).bind(&calibration.model_key).bind(&calibration.created_at).bind(json)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Calibrations of one model file, newest first.
    pub async fn calibrations_for(&self, model_id: &str, model_key: &str) -> DbResult<Vec<crate::calibration::Calibration>> {
        let rows: Vec<String> = sqlx::query_scalar("SELECT calibration_json FROM runtime_calibrations WHERE model_id=$1 AND model_key=$2 ORDER BY created_at DESC")
            .bind(model_id)
            .bind(model_key)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().filter_map(|json| serde_json::from_str(json).ok()).collect())
    }

    /// Keep what one model request carried and returned, then trim the table
    /// to the newest `MODEL_REQUEST_ROWS` rows and about `MODEL_REQUEST_BYTES`
    /// of text, oldest first.
    pub async fn record_model_request(&self, r: &ModelRequestRecord) -> DbResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO model_requests(id,conversation_id,owner_id,seq,kind,request_json,raw_output,finish_reason,outcome,failure,prompt_tokens,cached_tokens,generated_tokens,created_at)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
             ON CONFLICT(id) DO UPDATE SET conversation_id=excluded.conversation_id,owner_id=excluded.owner_id,seq=excluded.seq,kind=excluded.kind,
               request_json=excluded.request_json,raw_output=excluded.raw_output,finish_reason=excluded.finish_reason,outcome=excluded.outcome,
               failure=excluded.failure,prompt_tokens=excluded.prompt_tokens,cached_tokens=excluded.cached_tokens,
               generated_tokens=excluded.generated_tokens,created_at=excluded.created_at")
            .bind(&r.id).bind(&r.conversation_id).bind(&r.owner_id).bind(r.seq as i64).bind(&r.kind)
            .bind(&r.request_json).bind(&r.raw_output).bind(&r.finish_reason).bind(&r.outcome).bind(&r.failure)
            .bind(r.prompt_tokens as i64).bind(r.cached_tokens as i64).bind(r.generated_tokens as i64).bind(&r.created_at)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM model_requests WHERE id IN (SELECT id FROM model_requests ORDER BY created_at DESC, seq DESC OFFSET $1)")
            .bind(MODEL_REQUEST_ROWS as i64)
            .execute(&mut *tx)
            .await?;
        loop {
            let bytes: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(LENGTH(request_json) + LENGTH(raw_output)), 0)::BIGINT FROM model_requests")
                .fetch_one(&mut *tx)
                .await?;
            if bytes <= MODEL_REQUEST_BYTES as i64 {
                break;
            }
            let removed = sqlx::query("DELETE FROM model_requests WHERE id = (SELECT id FROM model_requests ORDER BY created_at ASC, seq ASC LIMIT 1)")
                .execute(&mut *tx)
                .await?;
            if removed.rows_affected() == 0 {
                break;
            }
        }
        tx.commit().await
    }

    /// Remember what the runtime reported for a model file (`model_key`: file
    /// name and size), so the model list can show it before the next load.
    pub async fn save_template_caps(&self, model_key: &str, caps: &crate::inference::TemplateCaps) -> DbResult<()> {
        let json = serde_json::to_string(caps).map_err(encode_error)?;
        sqlx::query("INSERT INTO model_template_caps(model_key,caps_json,updated_at) VALUES($1,$2,$3)
             ON CONFLICT(model_key) DO UPDATE SET caps_json=excluded.caps_json,updated_at=excluded.updated_at")
            .bind(model_key)
            .bind(json)
            .bind(chrono::Utc::now().to_rfc3339())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn template_caps_for(&self, model_key: &str) -> DbResult<Option<crate::inference::TemplateCaps>> {
        let json: Option<String> = sqlx::query_scalar("SELECT caps_json FROM model_template_caps WHERE model_key=$1")
            .bind(model_key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(json.and_then(|json| serde_json::from_str(&json).ok()))
    }

    /// Remember a fit decision under its key (`runtime_fit::fit_memory_key`).
    pub async fn save_fit_decision(&self, fit_key: &str, decision: &crate::runtime_fit::FitDecision) -> DbResult<()> {
        let json = serde_json::to_string(decision).map_err(encode_error)?;
        sqlx::query("INSERT INTO runtime_fit_decisions(fit_key,decision_json,updated_at) VALUES($1,$2,$3)
             ON CONFLICT(fit_key) DO UPDATE SET decision_json=excluded.decision_json,updated_at=excluded.updated_at")
            .bind(fit_key)
            .bind(json)
            .bind(chrono::Utc::now().to_rfc3339())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The remembered fit decision for a key; None when there is none or it no
    /// longer reads as a decision (a format from an older version).
    pub async fn fit_decision_for(&self, fit_key: &str) -> DbResult<Option<crate::runtime_fit::FitDecision>> {
        let json: Option<String> = sqlx::query_scalar("SELECT decision_json FROM runtime_fit_decisions WHERE fit_key=$1")
            .bind(fit_key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(json.and_then(|json| serde_json::from_str(&json).ok()))
    }

    /// The recorded model requests of one conversation, oldest first.
    pub async fn model_requests_for(&self, conversation_id: &str, limit: usize) -> DbResult<Vec<ModelRequestRecord>> {
        sqlx::query(
            "SELECT id,conversation_id,owner_id,seq,kind,request_json,raw_output,finish_reason,outcome,failure,prompt_tokens,cached_tokens,generated_tokens,created_at
             FROM model_requests WHERE conversation_id=$1 ORDER BY created_at ASC, seq ASC LIMIT $2",
        )
        .bind(conversation_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(model_request)
        .collect()
    }

    // ---- Stage 33 generation metrics ----

    pub async fn record_metric(&self, m: &GenerationMetric) -> DbResult<()> {
        let timing_json = m.timing.as_ref().map(serde_json::to_string).transpose().map_err(encode_error)?;
        sqlx::query("INSERT INTO generation_metrics(message_id,conversation_id,model_id,prompt_tokens,generated_tokens,gen_ms,ttft_ms,gen_tps,created_at,timing_json)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
             ON CONFLICT(message_id) DO UPDATE SET conversation_id=excluded.conversation_id,model_id=excluded.model_id,prompt_tokens=excluded.prompt_tokens,
               generated_tokens=excluded.generated_tokens,gen_ms=excluded.gen_ms,ttft_ms=excluded.ttft_ms,gen_tps=excluded.gen_tps,
               created_at=excluded.created_at,timing_json=excluded.timing_json")
            .bind(&m.message_id).bind(&m.conversation_id).bind(&m.model_id).bind(m.prompt_tokens as i64)
            .bind(m.generated_tokens as i64).bind(m.gen_ms as i64).bind(m.ttft_ms as i64).bind(m.gen_tps)
            .bind(&m.created_at).bind(timing_json)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn metrics_for(&self, conv: &str) -> DbResult<Vec<GenerationMetric>> {
        sqlx::query("SELECT message_id,conversation_id,model_id,prompt_tokens,generated_tokens,gen_ms,ttft_ms,gen_tps,created_at,timing_json FROM generation_metrics WHERE conversation_id=$1 ORDER BY seq")
            .bind(conv)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(metric)
            .collect()
    }

    // ---- Stage 14 fork: copy a thread into a new conversation ----

    /// Copy all messages (+attachment rows) from `src` to `dst`, all or nothing.
    /// Returns the copied message count. File bytes are shared on disk.
    pub async fn fork_messages(&self, src: &str, dst: &str, now: &str) -> DbResult<usize> {
        let messages = self.messages_for(src).await?;
        let attachments = self.attachments_for(src).await?;
        let mut tx = self.pool.begin().await?;
        for m in &messages {
            let new_id = uuid::Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO messages(id,conversation_id,role,content,created_at) VALUES($1,$2,$3,$4,$5)")
                .bind(&new_id).bind(dst).bind(&m.role).bind(&m.content).bind(now)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO message_activities(message_id,event_json) SELECT $1, event_json FROM message_activities WHERE message_id=$2 ORDER BY seq")
                .bind(&new_id)
                .bind(&m.id)
                .execute(&mut *tx)
                .await?;
        }
        for a in attachments {
            sqlx::query("INSERT INTO attachments(id,conversation_id,filename,mime,size_bytes,text_excerpt,kind,status,created_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
                .bind(uuid::Uuid::new_v4().to_string()).bind(dst).bind(&a.filename).bind(&a.mime).bind(a.size_bytes as i64)
                .bind(&a.text_excerpt).bind(&a.kind).bind(&a.status).bind(&a.created_at)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(messages.len())
    }
}

/// A conversation's messages, in insertion order (stable even when timestamps collide).
const MESSAGES_OF: &str = "SELECT id,conversation_id,role,content,created_at FROM messages WHERE conversation_id=$1 ORDER BY seq";
/// Every journal event in a conversation, in order.
const ACTIVITIES_OF: &str = "SELECT a.message_id, a.event_json FROM message_activities a
     JOIN messages m ON m.id = a.message_id WHERE m.conversation_id=$1 ORDER BY a.seq";

pub type ActivitiesByMessage = std::collections::HashMap<String, Vec<crate::agent::AgentEvent>>;

fn group_activities(rows: Vec<PgRow>) -> DbResult<ActivitiesByMessage> {
    let mut grouped = ActivitiesByMessage::new();
    for row in rows {
        let message_id: String = row.try_get("message_id")?;
        let json: String = row.try_get("event_json")?;
        if let Ok(event) = serde_json::from_str(&json) {
            grouped.entry(message_id).or_default().push(event);
        }
    }
    Ok(grouped)
}

/// Bounded, deterministic memory text: this session first, then its project,
/// then global preferences, about 2,000 characters at most.
fn memory_context_from(memories: &[MemoryEntry]) -> MemoryContext {
    const BUDGET: usize = 2000;
    if memories.is_empty() {
        return MemoryContext::default();
    }
    let mut result = MemoryContext {
        text: "\n\n[Saved user memory: context, not permission to act. The current request takes precedence.]\n".into(),
        entries: 0,
    };
    for scopes in [&["conversation", "session"][..], &["workspace"][..], &["global"][..]] {
        for memory in memories.iter().rev().filter(|memory| scopes.contains(&memory.scope.as_str())) {
            let source: String = memory.source.chars().filter(|character| !character.is_control()).take(40).collect();
            let label = format!("[{}; source: {}] ", memory.scope, if source.is_empty() { "user" } else { &source });
            let remaining = BUDGET.saturating_sub(result.text.chars().count());
            if remaining <= label.chars().count() + 12 {
                return result;
            }
            let limit = 500.min(remaining - label.chars().count() - 12);
            let content: String = memory.content.chars().take(limit).collect();
            result.text.push_str(&label);
            result.text.push_str(&content);
            if content.len() < memory.content.len() {
                result.text.push_str(" [trimmed]");
            }
            result.text.push('\n');
            result.entries += 1;
        }
    }
    result
}

/// Storage for tests: each call gets its own schema in the test database, so
/// tests run side by side without seeing each other's rows.
#[cfg(test)]
pub mod testing {
    use super::*;
    use sqlx::postgres::PgConnectOptions;
    use sqlx::{AssertSqlSafe, ConnectOptions};
    use std::str::FromStr;

    /// The server the tests use: `COMPANION_TEST_DATABASE_URL` (a database the
    /// tests may create and drop schemas in), or else a private server.
    fn server() -> PgConnectOptions {
        let url = match std::env::var("COMPANION_TEST_DATABASE_URL") {
            Ok(url) if !url.trim().is_empty() => url,
            _ => private_server().url("postgres"),
        };
        PgConnectOptions::from_str(&url).expect("COMPANION_TEST_DATABASE_URL is not a valid PostgreSQL URL")
    }

    /// A private server for the tests (runtime/pgsql, files in
    /// backend/target/test-postgres), running for as long as this test run:
    /// it ends with the run, even one cut short.
    pub fn private_server() -> crate::pgsql::Cluster {
        static SERVER: std::sync::OnceLock<crate::pgsql::Cluster> = std::sync::OnceLock::new();
        SERVER
            .get_or_init(|| {
                crate::terminal::end_children_with_this_process();
                let backend = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
                let dir = backend.join("target").join("test-postgres");
                // Throwaway data: no waiting for the disk (a third off the run).
                let fast = ["fsync=off", "synchronous_commit=off", "full_page_writes=off"];
                let cluster = crate::pgsql::Cluster::start(&backend.join("../runtime/pgsql/bin"), &dir.join("cluster"), &dir.join("cluster.log"), &fast)
                    .unwrap_or_else(|e| panic!("{e} (or set COMPANION_TEST_DATABASE_URL)"));
                #[cfg(unix)]
                if cluster.started {
                    cluster.stop_after(std::process::id());
                }
                cluster
            })
            .clone()
    }

    /// Connection options for one fresh, migrated schema. Runs on its own
    /// thread and runtime: callers are plain functions, often inside a
    /// `#[tokio::test]`, where blocking on the test's own runtime would hang.
    /// One schema is set up at a time, so clearing an earlier run's schemas
    /// can never catch a schema another test is creating.
    pub fn fresh_schema() -> PgConnectOptions {
        static SETUP: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
        let schema = format!("test_{}", uuid::Uuid::new_v4().simple());
        std::thread::spawn(move || {
            let mut cleared = SETUP.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("test runtime");
            let options = runtime.block_on(async {
                let server = server();
                let mut admin = server.connect().await.expect("cannot reach the test database");
                if !*cleared {
                    let old: Vec<String> = sqlx::query_scalar("SELECT nspname::TEXT FROM pg_namespace WHERE starts_with(nspname::TEXT, 'test_')")
                        .fetch_all(&mut admin)
                        .await
                        .expect("list old test schemas");
                    for name in old {
                        let _ = sqlx::query(AssertSqlSafe(format!("DROP SCHEMA IF EXISTS \"{name}\" CASCADE"))).execute(&mut admin).await;
                    }
                }
                sqlx::query(AssertSqlSafe(format!("CREATE SCHEMA \"{schema}\"")))
                    .execute(&mut admin)
                    .await
                    .expect("create test schema");
                let options = server.options([("search_path", schema.as_str())]);
                let mut connection = options.connect().await.expect("connect to test schema");
                MIGRATOR.run(&mut connection).await.expect("migrate test schema");
                options
            });
            *cleared = true;
            options
        })
        .join()
        .expect("test schema thread")
    }

    pub fn storage_on(options: PgConnectOptions) -> Storage {
        // No housekeeping task (lifetime and idle limits off), so a pool can be
        // made outside any async runtime, as plain #[test] functions do.
        Storage {
            pool: PgPoolOptions::new().max_connections(4).max_lifetime(None).idle_timeout(None).connect_lazy_with(options),
        }
    }

    /// A fresh, empty, migrated database for one test.
    pub fn storage() -> Storage {
        storage_on(fresh_schema())
    }

    impl Storage {
        /// The same database through a new pool, as a restarted program sees it.
        pub fn reopened(&self) -> Storage {
            storage_on((*self.pool.connect_options()).clone())
        }
    }
}

/// Rows kept in `model_requests`, newest first.
pub const MODEL_REQUEST_ROWS: usize = 300;
/// Text kept in `model_requests` (request plus output), about 50 MB.
pub const MODEL_REQUEST_BYTES: usize = 50_000_000;

/// What one model request carried and returned: enough to rebuild the exact
/// request that produced an output, and to replay it as a test fixture.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ModelRequestRecord {
    pub id: String,
    pub conversation_id: String,
    /// The reply message or agent run the request belonged to.
    pub owner_id: String,
    /// Order within the owner.
    pub seq: u32,
    /// chat | agent | classify | compaction
    pub kind: String,
    /// The request body as sent, with image data replaced by a placeholder.
    pub request_json: String,
    /// The visible text returned (native reasoning text is not kept).
    pub raw_output: String,
    pub finish_reason: Option<String>,
    /// completed | early_stopped | cancelled | failed
    pub outcome: String,
    pub failure: Option<String>,
    pub prompt_tokens: u32,
    pub cached_tokens: u32,
    pub generated_tokens: u32,
    pub created_at: String,
}

#[cfg(test)]
mod model_request_tests {
    use super::*;

    fn record(id: &str, at: &str, text_len: usize) -> ModelRequestRecord {
        ModelRequestRecord {
            id: id.into(),
            conversation_id: "c".into(),
            owner_id: "m".into(),
            seq: 0,
            kind: "chat".into(),
            request_json: "{}".into(),
            raw_output: "x".repeat(text_len),
            finish_reason: Some("stop".into()),
            outcome: "completed".into(),
            failure: None,
            prompt_tokens: 10,
            cached_tokens: 4,
            generated_tokens: 2,
            created_at: at.into(),
        }
    }

    fn at(i: usize) -> String {
        format!("2026-09-16T{:02}:{:02}:{:02}Z", i / 3600, (i / 60) % 60, i % 60)
    }

    #[tokio::test]
    async fn requests_round_trip_and_the_oldest_go_first_when_the_table_is_full() {
        let storage = testing::storage();
        for i in 0..(MODEL_REQUEST_ROWS + 5) {
            storage.record_model_request(&record(&format!("r{i:04}"), &at(i), 10)).await.unwrap();
        }
        let kept = storage.model_requests_for("c", 1_000).await.unwrap();
        assert_eq!(kept.len(), MODEL_REQUEST_ROWS);
        assert_eq!(kept[0].id, "r0005", "the five oldest were removed");
        let newest = MODEL_REQUEST_ROWS + 4;
        assert_eq!(kept.last().unwrap(), &record(&format!("r{newest:04}"), &at(newest), 10));
    }

    #[tokio::test]
    async fn the_text_cap_removes_the_oldest_large_records() {
        let storage = testing::storage();
        let big = MODEL_REQUEST_BYTES / 3 + 1;
        for i in 0..3 {
            storage.record_model_request(&record(&format!("big{i}"), &at(i), big)).await.unwrap();
        }
        let kept: Vec<String> = storage.model_requests_for("c", 10).await.unwrap().into_iter().map(|r| r.id).collect();
        assert_eq!(kept, vec!["big1", "big2"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_built_in_with_the_same_bytes_on_every_system() {
        // A carriage return would make the checksum differ between a Windows and a Linux checkout,
        // and sqlx refuses a database whose applied migration changed (`.gitattributes`: eol=lf).
        for migration in MIGRATOR.iter() {
            assert!(!migration.sql.as_str().contains('\r'), "migration {} has a carriage return", migration.version);
        }
    }

    fn conversation_named(id: &str) -> Conversation {
        Conversation {
            id: id.into(),
            title: "t".into(),
            model_id: "m".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            mode: "chat".into(),
            workspace: "".into(),
            reasoning_default: false,
            search_default: false,
            last_model: "".into(),
            priority: "normal".into(),
            related_to: "".into(),
            user_id: "local".into(),
        }
    }

    fn message_in(conversation: &str, id: &str, role: &str, content: &str, at: &str) -> Message {
        Message {
            id: id.into(),
            conversation_id: conversation.into(),
            role: role.into(),
            content: content.into(),
            created_at: at.into(),
        }
    }

    #[tokio::test]
    async fn a_fit_decision_is_remembered_by_key() {
        let storage = testing::storage();
        let decision = crate::runtime_fit::FitDecision {
            requested: 32_768,
            context: 22_528,
            cache: "q8_0".into(),
            margin: 256,
            micro_batch: Some(512),
            fit: crate::runtime_fit::Fit::AllLayers,
            placement: "gpu".into(),
            placement_at_default: "gpu".into(),
            draft_head_dropped: false,
            notes: vec!["searched".into()],
            micro_batch_note: None,
            free_vram_mib: Some(10_900),
        };
        assert_eq!(storage.fit_decision_for("key").await.unwrap(), None);
        storage.save_fit_decision("key", &decision).await.unwrap();
        assert_eq!(storage.fit_decision_for("key").await.unwrap(), Some(decision.clone()));
        let wider = crate::runtime_fit::FitDecision { context: 32_768, ..decision };
        storage.save_fit_decision("key", &wider).await.unwrap();
        assert_eq!(storage.fit_decision_for("key").await.unwrap(), Some(wider));
        assert_eq!(storage.fit_decision_for("other").await.unwrap(), None);
    }

    #[tokio::test]
    async fn task_continuation_is_durable_and_old_runs_cannot_overwrite_newer_goals() {
        let st = testing::storage();
        let mut context = TaskContext {
            conversation_id: "session".into(),
            workspace: "project".into(),
            task: "Fix the calculator".into(),
            run_id: "first".into(),
            status: "active".into(),
        };
        st.save_task_context(&context).await.unwrap();
        context.run_id = "newer".into();
        context.task = "Inspect the parser".into();
        st.save_task_context(&context).await.unwrap();
        st.finish_task_context("session", "first", "completed").await.unwrap();
        assert_eq!(st.task_context("session").await.unwrap().unwrap().status, "active");
        // A restart: the same database through a new pool, recovered.
        let st = st.reopened();
        st.recover_interrupted_activity().await.unwrap();
        let saved = st.task_context("session").await.unwrap().unwrap();
        assert_eq!(saved.run_id, "newer");
        assert_eq!(saved.task, "Inspect the parser");
        assert_eq!(saved.status, "interrupted");
        st.finish_task_context("session", "newer", "planned").await.unwrap();
        assert_eq!(st.task_context("session").await.unwrap().unwrap().status, "planned");
        st.delete_conversation("session").await.unwrap();
        assert!(st.task_context("session").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn compaction_is_a_derived_view_and_preserves_message_identity() {
        let storage = testing::storage();
        for i in 0..20 {
            let role = if i % 2 == 0 { "user" } else { "assistant" };
            storage
                .add_message(&message_in("context-test", &format!("m{i}"), role, &format!("Original message {i}"), &format!("2026-01-01T00:00:{i:02}Z")))
                .await
                .unwrap();
        }
        let original = serde_json::to_value(storage.messages_for("context-test").await.unwrap()).unwrap();
        let ids = (0..10).map(|i| format!("m{i}")).collect::<Vec<_>>();
        storage.save_context_summary("context-test", &ids, "A derived summary").await.unwrap();
        let context = storage.context_messages_for("context-test").await.unwrap();
        assert_eq!(context.len(), 11);
        assert_eq!(context[0].content, "A derived summary");
        assert_eq!(context[1].id, "m10");
        assert_eq!(serde_json::to_value(storage.messages_for("context-test").await.unwrap()).unwrap(), original);
        storage.update_message_content("context-test", "m0", "Edited source").await.unwrap();
        assert_eq!(
            storage.context_messages_for("context-test").await.unwrap().len(),
            20,
            "editing invalidates stale derived context"
        );
    }

    #[tokio::test]
    async fn activity_journal_keeps_verified_results_in_order() {
        let storage = testing::storage();
        let started = crate::agent::AgentEvent::activity("tool_started", crate::agent::AgentState::ExecutingTool, "Reading".into(), 1);
        let finished = crate::agent::AgentEvent::activity("tool_result", crate::agent::AgentState::Observing, "Read complete".into(), 1);
        storage.record_message_activity("reply", &started).await.unwrap();
        storage.record_message_activity("reply", &finished).await.unwrap();
        let events = storage.message_activities("reply").await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, "tool_started");
        assert_eq!(events[1].kind, "tool_result");
        assert!(storage.message_activities("other-reply").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn restart_marks_unfinished_journal_interrupted_without_losing_partial_text() {
        let storage = testing::storage();
        storage
            .add_message(&message_in("session", "interrupted", "assistant", "Partial response", "original timestamp"))
            .await
            .unwrap();
        storage
            .record_message_activity(
                "interrupted",
                &crate::agent::AgentEvent::activity("tool_started", crate::agent::AgentState::ExecutingTool, "Reading".into(), 2),
            )
            .await
            .unwrap();
        storage.recover_interrupted_activity().await.unwrap();
        let events = storage.message_activities("interrupted").await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].state, crate::agent::AgentState::Cancelled);
        assert_eq!(storage.get_message("session", "interrupted").await.unwrap().unwrap().content, "Partial response");
        storage.recover_interrupted_activity().await.unwrap();
        assert_eq!(storage.message_activities("interrupted").await.unwrap().len(), 2, "restart recovery is idempotent");
    }

    #[tokio::test]
    async fn conversation_roundtrip_survives_restart_semantics() {
        let s = testing::storage();
        s.create_conversation(&conversation_named("c1")).await.unwrap();
        s.add_message(&message_in("c1", "m1", "user", "hi", "2026-01-01T00:00:01Z")).await.unwrap();
        assert_eq!(s.list_conversations("local").await.unwrap().len(), 1);
        assert_eq!(s.messages_for("c1").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn calibrations_are_kept_newest_first_per_model_file() {
        use crate::calibration::*;
        let storage = testing::storage();
        let make = |id: &str, at: &str, key: &str| Calibration {
            id: id.into(),
            model_id: "m".into(),
            model_key: key.into(),
            created_at: at.into(),
            environment: Environment::default(),
            plan: plan(8, 8, Placement::Cpu, 0),
            measurements: vec![],
            profiles: vec![],
            regression: None,
            micro_batches: vec![],
            micro_batch: None,
        };
        storage.save_calibration(&make("old", "2026-09-01T00:00:00Z", "m.gguf:1")).await.unwrap();
        storage.save_calibration(&make("new", "2026-09-02T00:00:00Z", "m.gguf:1")).await.unwrap();
        storage.save_calibration(&make("other-file", "2026-09-03T00:00:00Z", "m.gguf:2")).await.unwrap();
        let found = storage.calibrations_for("m", "m.gguf:1").await.unwrap();
        assert_eq!(found.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["new", "old"]);
    }

    #[tokio::test]
    async fn records_survive_reopen_and_delete_cascades_messages() {
        let s = testing::storage();
        s.create_conversation(&conversation_named("c9")).await.unwrap();
        s.add_message(&message_in("c9", "m9", "user", "hi", "2026-01-01T00:00:01Z")).await.unwrap();
        assert!(s.get_conversation("c9").await.unwrap().is_some());
        let s = s.reopened();
        assert_eq!(s.messages_for("c9").await.unwrap().len(), 1);
        assert!(s.delete_conversation("c9").await.unwrap());
        assert!(s.get_conversation("c9").await.unwrap().is_none());
        assert!(s.messages_for("c9").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn two_edits_from_the_same_read_both_stay() {
        let s = testing::storage();
        s.create_conversation(&conversation_named("c")).await.unwrap();
        let read = s.get_conversation("c").await.unwrap().unwrap();
        let reasoning = Conversation { reasoning_default: true, ..read.clone() };
        let search = Conversation { search_default: true, ..read.clone() };
        s.update_conversation(&read, &reasoning).await.unwrap();
        s.update_conversation(&read, &search).await.unwrap();
        s.set_last_model("c", "model-b").await.unwrap();
        s.update_conversation(&read, &Conversation { title: "Budget".into(), ..read.clone() }).await.unwrap();
        let saved = s.get_conversation("c").await.unwrap().unwrap();
        assert!(saved.reasoning_default && saved.search_default, "the second edit did not undo the first");
        assert_eq!((saved.title.as_str(), saved.last_model.as_str()), ("Budget", "model-b"));
    }

    #[tokio::test]
    async fn re_indexing_a_file_at_once_twice_leaves_one_set_of_chunks() {
        let s = testing::storage();
        let pieces: Vec<String> = (0..20).map(|i| format!("piece {i}")).collect();
        let (a, b) = tokio::join!(s.replace_knowledge_path("ws", "notes.md", &pieces), s.replace_knowledge_path("ws", "notes.md", &pieces));
        a.unwrap();
        b.unwrap();
        assert_eq!(s.knowledge_paths("ws").await.unwrap(), vec![("notes.md".to_string(), 20)]);
    }

    #[tokio::test]
    async fn a_reader_never_sees_a_reply_half_finished() {
        let s = testing::storage();
        s.create_conversation(&conversation_named("c")).await.unwrap();
        let failed = crate::agent::AgentEvent::new(crate::agent::AgentState::Failed, "No inference running.".into(), 0);
        for i in 0..150 {
            let id = format!("m{i}");
            s.add_message(&message_in("c", &id, "assistant", "Work is starting…", "now")).await.unwrap();
            let (finished, read) = tokio::join!(s.finish_message("c", &id, &failed), s.messages_with_activities("c"));
            finished.unwrap();
            let (messages, journals) = read.unwrap();
            for message in messages {
                let ended = journals.get(&message.id).is_some_and(|events| !events.is_empty());
                assert_eq!(ended, message.content != "Work is starting…", "read {i}: {} has {:?} with text {:?}", message.id, journals.get(&message.id), message.content);
            }
        }
    }

    #[tokio::test]
    async fn a_finished_reply_and_its_last_event_are_saved_together() {
        let s = testing::storage();
        seed_thread(&s).await;
        let event = crate::agent::AgentEvent::new(crate::agent::AgentState::Failed, "No inference running.".into(), 0);
        s.finish_message("ce", "e3", &event).await.unwrap();
        assert_eq!(s.get_message("ce", "e3").await.unwrap().unwrap().content, "No inference running.");
        assert_eq!(s.message_activities("e3").await.unwrap().last().unwrap().state, crate::agent::AgentState::Failed);
    }

    async fn seed_thread(s: &Storage) {
        s.create_conversation(&Conversation { id: "ce".into(), ..conversation_named("ce") }).await.unwrap();
        for (i, role) in ["user", "assistant", "user", "assistant"].iter().enumerate() {
            s.add_message(&message_in("ce", &format!("e{i}"), role, &format!("msg{i}"), &format!("2026-01-01T00:00:0{i}Z")))
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn memory_scopes_are_visible_selectively() {
        let s = testing::storage();
        let now = "2026-01-01T00:00:00Z";
        for (id, scope, scope_id) in [("g1", "global", ""), ("c1", "conversation", "ce"), ("w1", "workspace", "ws1"), ("o1", "conversation", "other")] {
            s.add_memory(&MemoryEntry {
                id: id.into(),
                scope_id: scope_id.into(),
                scope: scope.into(),
                content: format!("mem {id}"),
                source: "user".into(),
                created_at: now.into(),
                last_used: now.into(),
                user_id: "local".into(),
            })
            .await
            .unwrap();
        }
        let vis: Vec<String> = s.memories_for("local", "ce", "ws1").await.unwrap().into_iter().map(|m| m.id).collect();
        assert!(vis.contains(&"g1".to_string()));
        assert!(vis.contains(&"c1".to_string()));
        assert!(vis.contains(&"w1".to_string()));
        assert!(!vis.contains(&"o1".to_string()), "other conversations must not leak: {vis:?}");
        assert!(s.delete_memory("g1").await.unwrap());
        assert!(!s.delete_memory("g1").await.unwrap());
    }

    #[tokio::test]
    async fn prompt_memory_is_scoped_bounded_and_labels_provenance() {
        let storage = testing::storage();
        for (id, scope, scope_id) in [
            ("own", "conversation", "session-a"),
            ("legacy-own", "session", "session-a"),
            ("project", "workspace", "workspace-a"),
            ("global", "global", ""),
            ("foreign-session", "conversation", "session-b"),
            ("foreign-project", "workspace", "workspace-b"),
            ("not-a-session", "session", "workspace-a"),
        ] {
            storage
                .add_memory(&MemoryEntry {
                    id: id.into(),
                    scope: scope.into(),
                    scope_id: scope_id.into(),
                    content: format!("fact-{id}"),
                    source: "user".into(),
                    created_at: "now".into(),
                    last_used: "now".into(),
                    user_id: "local".into(),
                })
                .await
                .unwrap();
        }
        let context = storage.memory_context("local", "session-a", "workspace-a").await.unwrap();
        assert_eq!(context.entries, 4);
        assert!(
            context.text.contains("fact-own")
                && context.text.contains("fact-legacy-own")
                && context.text.contains("fact-project")
                && context.text.contains("fact-global")
        );
        assert!(!context.text.contains("fact-foreign") && !context.text.contains("fact-not-a-session"));
        assert!(context.text.contains("source: user"));
        storage
            .add_memory(&MemoryEntry {
                id: "large".into(),
                scope: "conversation".into(),
                scope_id: "session-a".into(),
                content: "界".repeat(5000),
                source: "user".into(),
                created_at: "now".into(),
                last_used: "now".into(),
                user_id: "local".into(),
            })
            .await
            .unwrap();
        let context = storage.memory_context("local", "session-a", "workspace-a").await.unwrap();
        assert!(context.text.chars().count() <= 2000);
        assert!(context.text.contains("[trimmed]"));
        assert_eq!(
            context.text,
            storage.memory_context("local", "session-a", "workspace-a").await.unwrap().text,
            "context selection is deterministic"
        );
    }

    #[tokio::test]
    async fn edit_truncates_later_messages() {
        let s = testing::storage();
        seed_thread(&s).await;
        assert!(s.update_message_content("ce", "e1", "edited").await.unwrap());
        assert_eq!(s.get_message("ce", "e1").await.unwrap().unwrap().content, "edited");
        assert_eq!(s.delete_messages_after("ce", "e1").await.unwrap(), 2);
        let rest = s.messages_for("ce").await.unwrap();
        assert_eq!(rest.len(), 2);
        assert_eq!(rest[1].content, "edited");
    }

    #[tokio::test]
    async fn attachments_and_tool_audit_roundtrip() {
        let s = testing::storage();
        seed_thread(&s).await;
        s.add_attachment(&Attachment {
            id: "a1".into(),
            conversation_id: "ce".into(),
            filename: "notes.txt".into(),
            mime: "text/plain".into(),
            size_bytes: 3,
            text_excerpt: "abc".into(),
            kind: "text".into(),
            status: "ready".into(),
            created_at: "2026-01-01T00:00:05Z".into(),
        })
        .await
        .unwrap();
        assert_eq!(s.attachments_for("ce").await.unwrap().len(), 1);
        s.record_tool_execution(&ToolExecution {
            id: "t1".into(),
            conversation_id: "ce".into(),
            tool: "read_file".into(),
            args: "{}".into(),
            result: "ok".into(),
            approved: true,
            approval: "allowed by the permission mode".into(),
            created_at: "2026-01-01T00:00:06Z".into(),
        }, &crate::audit::Who::default())
        .await
        .unwrap();
        let execs = s.tool_executions_for("ce", 10).await.unwrap();
        assert_eq!(execs.len(), 1);
        assert!(execs[0].approved);
        assert_eq!(execs[0].approval, "allowed by the permission mode");
    }

    #[tokio::test]
    async fn a_fork_copies_messages_journals_and_attachments_in_order() {
        let s = testing::storage();
        seed_thread(&s).await;
        let event = crate::agent::AgentEvent::activity("status", crate::agent::AgentState::Completed, "done".into(), 1);
        s.record_message_activity("e1", &event).await.unwrap();
        s.add_attachment(&Attachment {
            id: "a1".into(),
            conversation_id: "ce".into(),
            filename: "notes.txt".into(),
            mime: "text/plain".into(),
            size_bytes: 3,
            text_excerpt: "abc".into(),
            kind: "text".into(),
            status: "ready".into(),
            created_at: "now".into(),
        })
        .await
        .unwrap();
        assert_eq!(s.fork_messages("ce", "copy", "later").await.unwrap(), 4);
        let copied = s.messages_for("copy").await.unwrap();
        assert_eq!(copied.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(), vec!["msg0", "msg1", "msg2", "msg3"]);
        let source: Vec<String> = s.messages_for("ce").await.unwrap().into_iter().map(|m| m.id).collect();
        assert!(copied.iter().all(|m| m.created_at == "later" && !source.contains(&m.id)), "new ids, new time");
        assert_eq!(s.message_activities(&copied[1].id).await.unwrap().len(), 1);
        assert_eq!(s.attachments_for("copy").await.unwrap()[0].filename, "notes.txt");
        assert_eq!(s.messages_for("ce").await.unwrap().len(), 4, "the source is untouched");
    }

    #[tokio::test]
    async fn artifact_roundtrip() {
        let s = testing::storage();
        seed_thread(&s).await;
        s.record_artifact(&ArtifactRow {
            id: "ar1".into(),
            conversation_id: "ce".into(),
            filename: "f.csv".into(),
            path: "/tmp/f.csv".into(),
            mime: "text/csv".into(),
            size_bytes: 12,
            created_at: "2026-01-01T00:00:07Z".into(),
        })
        .await
        .unwrap();
        let all = s.artifacts_for("ce").await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(s.get_artifact("ar1").await.unwrap().unwrap().mime, "text/csv");
        assert!(s.get_artifact("nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn metrics_keep_the_output_timing_when_there_is_one() {
        let storage = testing::storage();
        let base = GenerationMetric {
            message_id: "old".into(),
            conversation_id: "conv".into(),
            model_id: "model".into(),
            prompt_tokens: 100,
            generated_tokens: 20,
            gen_ms: 10_000,
            ttft_ms: 2_000,
            gen_tps: 2.0,
            timing: None,
            created_at: "then".into(),
        };
        storage.record_metric(&base).await.unwrap();
        let timing = crate::inference::OutputTiming {
            output_tps: Some(20.0),
            output_tokens: 20,
            output_ms: 1000,
            first_visible_ms: Some(5000),
            thinking_ms: Some(2000),
            total_ms: 6300,
            token_basis: "tokenizer".into(),
            ..crate::inference::OutputTiming::default()
        };
        storage
            .record_metric(&GenerationMetric {
                message_id: "new".into(),
                generated_tokens: 200,
                gen_ms: 6300,
                ttft_ms: 5000,
                gen_tps: 20.0,
                timing: Some(timing),
                created_at: "now".into(),
                ..base
            })
            .await
            .unwrap();
        let metrics = storage.metrics_for("conv").await.unwrap();
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].gen_tps, 2.0);
        assert!(metrics[0].timing.is_none(), "a rate without timing must not gain a visible-output label");
        let output = metrics[1].timing.as_ref().unwrap();
        assert_eq!(output.basis, "visible_output_v1");
        assert_eq!(output.output_tps, Some(20.0));
        assert_eq!(output.output_tokens, 20);
        assert_eq!(output.thinking_ms, Some(2000));
    }
}
