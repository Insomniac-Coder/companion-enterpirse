//! Audit records (Phase 1, task 8): one row per model request, tool call, web search, approval,
//! admin or settings change, and sign-in, saying who, when, from which device, what, which model,
//! how many tokens and how it was allowed. Rows are only added (`migrations/0006_audit.sql`
//! refuses changes); the fingerprint chain, retention and the viewer follow in Phase 7.
//!
//! Message text is not kept here. Tool arguments, queries and admin request bodies are, masked
//! (`logfile::masked`) and shortened to `EXCERPT_CHARS`.

use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;
use sqlx::Row;

use crate::api::AppState;
use crate::auth::Caller;

/// The most of one argument, query or body an audit record keeps.
pub const EXCERPT_CHARS: usize = 1000;
/// Admin request bodies up to this size are read for the record; larger ones are noted by size.
const BODY_BYTES: usize = 64 * 1024;

/// Who did something, and from where.
#[derive(Clone, Debug, Default)]
pub struct Who {
    pub user_id: String,
    /// local, session, api key, agent, system.
    pub via: String,
    pub address: String,
    pub device: String,
}

impl Who {
    pub fn of(caller: &Caller) -> Who {
        Who { user_id: caller.id.clone(), via: caller.via.into(), address: caller.address.clone(), device: caller.device.clone() }
    }

    /// A run started by `self`, acting on its own for them.
    pub fn agent(&self) -> Who {
        Who { via: "agent".into(), ..self.clone() }
    }

    /// The server's own work (a model's tool check after loading).
    pub fn system(what: &str) -> Who {
        Who { user_id: "system".into(), via: "system".into(), device: what.into(), ..Who::default() }
    }
}

/// One audit record; `action` is required, the rest as the action has them.
#[derive(Debug, Default)]
pub struct Record {
    pub action: &'static str,
    pub target: String,
    pub model: String,
    pub prompt_tokens: i64,
    pub generated_tokens: i64,
    pub allowed_by: String,
    pub outcome: String,
    pub detail: serde_json::Value,
}

/// `text` masked and cut to `EXCERPT_CHARS`.
pub fn excerpt(text: &str) -> String {
    let masked = crate::logfile::masked(text);
    let mut kept: String = masked.chars().take(EXCERPT_CHARS).collect();
    if kept.len() < masked.len() {
        kept.push('…');
    }
    kept
}

/// Add `record` inside `executor`'s transaction or connection.
pub async fn insert<'e, E: sqlx::PgExecutor<'e>>(executor: E, who: &Who, record: &Record) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_records (user_id, via, address, device, action, target, model, prompt_tokens, generated_tokens, allowed_by, outcome, detail)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(&who.user_id)
    .bind(&who.via)
    .bind(&who.address)
    .bind(&who.device)
    .bind(record.action)
    .bind(&record.target)
    .bind(&record.model)
    .bind(record.prompt_tokens)
    .bind(record.generated_tokens)
    .bind(&record.allowed_by)
    .bind(&record.outcome)
    .bind(if record.detail.is_null() { serde_json::json!({}) } else { record.detail.clone() })
    .execute(executor)
    .await
    .map(|_| ())
}

/// Add `record`; a failure is logged, never passed to the person's request.
pub async fn write(state: &AppState, who: &Who, record: Record) {
    if let Err(error) = insert(state.storage.pool(), who, &record).await {
        tracing::error!(action = record.action, "could not write the audit record: {error}");
    }
}

/// The connection's address and the browser's or app's name. Behind a load balancer the address
/// is the balancer's until forwarded addresses are trusted (Phase 8).
pub fn origin(
    connection: Option<&axum::extract::ConnectInfo<std::net::SocketAddr>>,
    headers: &axum::http::HeaderMap,
) -> (String, String) {
    let address = connection.map(|info| info.0.ip().to_string()).unwrap_or_default();
    let device = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(|agent| agent.chars().take(200).collect())
        .unwrap_or_default();
    (address, device)
}

/// Set on the response by the admin route guards (`roles.rs`), so `changes` records every call to
/// an admin route, allowed or refused, without its own list of them.
#[derive(Clone, Copy)]
pub struct AdminRoute;

/// What kind of change a non-GET request to `path` is, if it is one the audit keeps.
fn change_kind(path: &str, admin_route: bool) -> Option<&'static str> {
    if admin_route || path.starts_with("/api/admin/") {
        Some("admin_change")
    } else if path == "/api/settings" || path == "/api/permissions/mode" {
        Some("settings_change")
    } else if path == "/api/auth/logout" {
        Some("sign_out")
    } else if path.starts_with("/api/me/api-keys") {
        Some("api_key")
    } else {
        None
    }
}

/// Records admin and settings changes, sign-outs and API key changes: who, the route, what was
/// sent (masked, shortened) and the answer's status.
pub async fn changes(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if matches!(*request.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        return next.run(request).await;
    }
    let Some(caller) = request.extensions().get::<Caller>().cloned() else {
        return next.run(request).await;
    };
    let route = format!("{} {}", request.method(), request.uri().path());
    let path = request.uri().path().to_string();
    // Known for a body sent with its length (what browsers and the apps do); a streamed one of
    // unknown length is not read, so the handler still gets all of it.
    let size = axum::body::HttpBody::size_hint(request.body()).exact().map(|size| size as usize);
    let (request, body) = match size {
        Some(size) if size <= BODY_BYTES => {
            let (parts, body) = request.into_parts();
            let bytes = axum::body::to_bytes(body, BODY_BYTES).await.unwrap_or_default();
            let text = excerpt(&String::from_utf8_lossy(&bytes));
            (Request::from_parts(parts, axum::body::Body::from(bytes)), text)
        }
        Some(size) => (request, format!("({size} bytes, not kept)")),
        None => (request, String::new()),
    };
    let response = next.run(request).await;
    let admin_route = response.extensions().get::<AdminRoute>().is_some();
    if let Some(action) = change_kind(&path, admin_route) {
        let allowed_by = if admin_route { "platform admin role" } else { "" };
        write(
            &state,
            &Who::of(&caller),
            Record {
                action,
                target: route,
                allowed_by: allowed_by.into(),
                outcome: response.status().as_u16().to_string(),
                detail: serde_json::json!({ "body": body }),
                ..Record::default()
            },
        )
        .await;
    }
    response
}

/// GET /api/admin/audit?before=<seq>&limit=<n>: the newest records first, for a platform admin or
/// an auditor. The viewer with filters follows in Phase 7.
pub async fn list(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<ListQuery>,
) -> Result<axum::Json<serde_json::Value>, crate::api::ApiError> {
    let rows = sqlx::query(
        "SELECT seq, at::TEXT AS at, user_id, via, address, device, action, target, model, prompt_tokens, generated_tokens, allowed_by, outcome, detail
         FROM audit_records WHERE seq < $1 ORDER BY seq DESC LIMIT $2",
    )
    .bind(query.before.unwrap_or(i64::MAX))
    .bind(query.limit.unwrap_or(100).clamp(1, 500))
    .fetch_all(state.storage.pool())
    .await
    .map_err(|error| crate::api::ApiError::internal(format!("storage error: {error}")))?;
    let records: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "seq": row.get::<i64, _>("seq"),
                "at": row.get::<String, _>("at"),
                "user_id": row.get::<String, _>("user_id"),
                "via": row.get::<String, _>("via"),
                "address": row.get::<String, _>("address"),
                "device": row.get::<String, _>("device"),
                "action": row.get::<String, _>("action"),
                "target": row.get::<String, _>("target"),
                "model": row.get::<String, _>("model"),
                "prompt_tokens": row.get::<i64, _>("prompt_tokens"),
                "generated_tokens": row.get::<i64, _>("generated_tokens"),
                "allowed_by": row.get::<String, _>("allowed_by"),
                "outcome": row.get::<String, _>("outcome"),
                "detail": row.get::<serde_json::Value, _>("detail"),
            })
        })
        .collect();
    Ok(axum::Json(serde_json::json!({ "records": records })))
}

#[derive(serde::Deserialize)]
pub struct ListQuery {
    before: Option<i64>,
    limit: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::tests::{call, json, person, server, status};
    use crate::roles::{AUDITOR, PLATFORM_ADMIN};
    use axum::http::StatusCode;

    #[derive(Debug, sqlx::FromRow)]
    struct Kept {
        user_id: String,
        device: String,
        target: String,
        allowed_by: String,
        outcome: String,
        detail: serde_json::Value,
    }

    async fn kept(state: &AppState, action: &str) -> Vec<Kept> {
        sqlx::query_as("SELECT user_id, device, target, allowed_by, outcome, detail FROM audit_records WHERE action = $1 ORDER BY seq")
            .bind(action)
            .fetch_all(state.storage.pool())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn changes_and_tool_calls_record_who_what_and_how() {
        let (app, state) = server();
        let (ada_id, ada) = person(&state, "ada", &[]).await;
        let (boss_id, boss) = person(&state, "boss", &[PLATFORM_ADMIN]).await;
        let (_, audrey) = person(&state, "audrey", &[AUDITOR]).await;

        // An admin change (its secret masked), a refused one, and an admin route outside /api/admin/.
        let mut change = call("PUT", "/api/admin/settings/company", &boss, Some(serde_json::json!({ "settings": { "search": { "brave_key": "BSAsecret123456" } } })));
        change.headers_mut().insert(axum::http::header::USER_AGENT, "Companion test".parse().unwrap());
        assert_eq!(status(&app, change).await, StatusCode::OK);
        assert_eq!(status(&app, call("PUT", "/api/admin/settings/company", &ada, Some(serde_json::json!({ "settings": {} })))).await, StatusCode::FORBIDDEN);
        assert_eq!(status(&app, call("POST", "/api/models/scan", &ada, None)).await, StatusCode::FORBIDDEN);
        let admin = kept(&state, "admin_change").await;
        let summary: Vec<(&str, &str, &str)> = admin.iter().map(|r| (r.user_id.as_str(), r.target.as_str(), r.outcome.as_str())).collect();
        assert_eq!(summary, [
            (boss_id.as_str(), "PUT /api/admin/settings/company", "200"),
            (ada_id.as_str(), "PUT /api/admin/settings/company", "403"),
            (ada_id.as_str(), "POST /api/models/scan", "403"),
        ]);
        assert_eq!((admin[0].device.as_str(), admin[0].allowed_by.as_str()), ("Companion test", "platform admin role"));
        let body = admin[0].detail["body"].as_str().unwrap();
        assert!(body.contains("brave_key") && !body.contains("BSAsecret123456"), "{body}");

        // Tool calls: refused without approval, then run with it.
        let folder = std::env::temp_dir().join(format!("companion-audit-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&folder).unwrap();
        let folder = folder.to_string_lossy().into_owned();
        assert_eq!(status(&app, call("POST", "/api/workspaces", &ada, Some(serde_json::json!({ "name": "p", "path": folder })))).await, StatusCode::OK);
        let write = |approved: bool| {
            let body = serde_json::json!({ "workspace": folder, "tool": "write_file", "args": { "path": "n.txt", "content": "x" }, "approved_once": approved });
            call("POST", "/api/tools/execute", &ada, Some(body))
        };
        assert_eq!(status(&app, write(false)).await, StatusCode::FORBIDDEN);
        assert_eq!(status(&app, write(true)).await, StatusCode::OK);
        let tools = kept(&state, "tool_call").await;
        let summary: Vec<(&str, &str, &str)> = tools.iter().map(|r| (r.user_id.as_str(), r.allowed_by.as_str(), r.outcome.as_str())).collect();
        assert_eq!(summary, [(ada_id.as_str(), "refused: needs approval", "refused"), (ada_id.as_str(), "approved by the user", "ran")]);
        assert_eq!(tools[1].detail["tool"], "write_file");
        assert!(tools[1].detail["args"].as_str().unwrap().contains("n.txt"));
        let _ = std::fs::remove_dir_all(&folder);

        // A person's own settings change.
        assert_eq!(status(&app, call("PUT", "/api/permissions/mode", &ada, Some(serde_json::json!({ "mode": "accept_edits" })))).await, StatusCode::OK);
        let own = kept(&state, "settings_change").await;
        assert_eq!((own.len(), own[0].user_id.as_str(), own[0].target.as_str()), (1, ada_id.as_str(), "PUT /api/permissions/mode"));

        // An auditor reads them, newest first; a person cannot; reading adds nothing.
        let before: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_records").fetch_one(state.storage.pool()).await.unwrap();
        let read = json(&app, call("GET", "/api/admin/audit?limit=2", &audrey, None)).await;
        assert_eq!(read["records"].as_array().unwrap().len(), 2);
        assert_eq!(read["records"][0]["action"], "settings_change");
        assert_eq!(status(&app, call("GET", "/api/admin/audit", &ada, None)).await, StatusCode::FORBIDDEN);
        let after: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_records").fetch_one(state.storage.pool()).await.unwrap();
        assert_eq!(before, after);

        // Rows only grow.
        let pool = state.storage.pool();
        assert!(sqlx::query("UPDATE audit_records SET outcome = 'changed'").execute(pool).await.is_err());
        assert!(sqlx::query("DELETE FROM audit_records").execute(pool).await.is_err());
        assert!(sqlx::query("TRUNCATE audit_records").execute(pool).await.is_err());
    }

    #[tokio::test]
    async fn every_model_request_is_recorded_with_its_tokens_even_when_its_text_is_not() {
        let state = AppState::new_stub();
        state.settings.write().await.privacy.record_model_requests = false;
        let who = Who { user_id: "local".into(), via: "local".into(), ..Who::default() };
        let recorder = crate::api::request_recorder(&state, &who, "c1", "m1", "chat");
        recorder(crate::llamaserver::RecordedRequest {
            body: serde_json::json!({}),
            output: "hello".into(),
            finish_reason: Some("stop".into()),
            outcome: "completed",
            failure: None,
            prompt_tokens: 12,
            cached_tokens: 0,
            generated_tokens: 3,
        });
        let mut found = None;
        for _ in 0..100 {
            found = sqlx::query_as::<_, (String, String, i64, i64, String, serde_json::Value)>(
                "SELECT user_id, target, prompt_tokens, generated_tokens, outcome, detail FROM audit_records WHERE action = 'model_request'",
            )
            .fetch_optional(state.storage.pool())
            .await
            .unwrap();
            if found.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let (user, target, prompt, generated, outcome, detail) = found.expect("an audit record");
        assert_eq!((user.as_str(), target.as_str(), prompt, generated, outcome.as_str()), ("local", "c1", 12, 3, "completed"));
        assert_eq!(detail["kind"], "chat");
        let texts: i64 = sqlx::query_scalar("SELECT count(*) FROM model_requests").fetch_one(state.storage.pool()).await.unwrap();
        assert_eq!(texts, 0, "the text is kept only when Privacy says so");
    }

    #[test]
    fn excerpts_are_masked_and_short() {
        assert_eq!(excerpt("Authorization: Bearer abc.def"), "Authorization: Bearer ***");
        let long = "x".repeat(EXCERPT_CHARS + 5);
        assert_eq!(excerpt(&long).chars().count(), EXCERPT_CHARS + 1);
    }
}
