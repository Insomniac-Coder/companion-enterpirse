//! An owner on every record (Phase 1, task 5).
//!
//! Conversations, projects and saved memories name their person (`user_id`); everything else
//! belongs to a conversation or a project, and an agent run to the person who started it. A record
//! of someone else's is "not found": whether it exists is not theirs to know. Platform admins are no
//! exception: running the server is not reading people's work.
//!
//! Every route that names a record in its path does it the same way (`/api/conversations/<id>/…`,
//! `/api/workspaces/<id>/…`), so [`owned_paths`] checks them all before any handler runs, including
//! routes added later under those paths. Handlers still check what a request names in its body
//! ([`conversation`], [`workspace`]), scope their lists to the caller, and stamp new records with them.

use crate::api::{ApiError, AppState};
use crate::auth::Caller;
use crate::storage::{Conversation, Workspace};
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// What a path names, by its first segments.
#[derive(Debug, PartialEq)]
enum Named {
    Conversation(String),
    Workspace(String),
    Memory(String),
    Run(String),
    Artifact(String),
}

/// The record a path names, if it names one.
fn named(path: &str) -> Option<Named> {
    let segments: Vec<String> = path
        .split('/')
        .map(|segment| percent_encoding::percent_decode_str(segment).decode_utf8_lossy().into_owned())
        .collect();
    let id = |index: usize| segments.get(index).filter(|id| !id.is_empty()).cloned();
    match segments.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["", "api", "conversations", ..] => id(3).map(Named::Conversation),
        // `recovery` lists the caller's own; every other session path is a conversation id.
        ["", "api", "sessions", "recovery", ..] => None,
        ["", "api", "sessions", ..] => id(3).map(Named::Conversation),
        ["", "api", "workspaces", ..] => id(3).map(Named::Workspace),
        ["", "api", "memory", ..] => id(3).map(Named::Memory),
        ["", "api", "agent", "runs", ..] => id(4).map(Named::Run),
        ["", "api", "artifacts", ..] => id(3).map(Named::Artifact),
        _ => None,
    }
}

fn not_found(what: &str) -> ApiError {
    ApiError::not_found(format!("{what} not found"))
}

fn storage_error(error: sqlx::Error) -> ApiError {
    ApiError::internal(format!("storage error: {error}"))
}

/// The caller's conversation `id`.
pub async fn conversation(state: &AppState, caller: &Caller, id: &str) -> Result<Conversation, ApiError> {
    match state.storage.get_conversation(id).await.map_err(storage_error)? {
        Some(found) if found.user_id == caller.id => Ok(found),
        _ => Err(not_found("conversation")),
    }
}

/// The caller's project `id`.
pub async fn workspace(state: &AppState, caller: &Caller, id: &str) -> Result<Workspace, ApiError> {
    match state.storage.get_workspace(id).await.map_err(storage_error)? {
        Some(found) if found.user_id == caller.id => Ok(found),
        _ => Err(not_found("project")),
    }
}

async fn check(state: &AppState, caller: &Caller, record: Named) -> Result<(), ApiError> {
    match record {
        Named::Conversation(id) => conversation(state, caller, &id).await.map(drop),
        Named::Workspace(id) => workspace(state, caller, &id).await.map(drop),
        Named::Memory(id) => match state.storage.get_memory(&id).await.map_err(storage_error)? {
            Some(memory) if memory.user_id == caller.id => Ok(()),
            _ => Err(not_found("memory")),
        },
        Named::Artifact(id) => {
            let artifact = state.storage.get_artifact(&id).await.map_err(storage_error)?.ok_or_else(|| not_found("artifact"))?;
            conversation(state, caller, &artifact.conversation_id).await.map(drop).map_err(|_| not_found("artifact"))
        }
        Named::Run(id) => match state.agents.read().await.get(&id) {
            Some(run) if run.spec.user_id == caller.id => Ok(()),
            _ => Err(not_found("run")),
        },
    }
}

/// Before any handler: a path that names a record must name one of the caller's.
pub async fn owned_paths(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let Some(record) = named(request.uri().path()) else {
        return next.run(request).await;
    };
    let Some(caller) = request.extensions().get::<Caller>().cloned() else {
        return not_found("record").into_response();
    };
    match check(&state, &caller, record).await {
        Ok(()) => next.run(request).await,
        Err(error) => error.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_that_name_a_record_are_recognised() {
        assert_eq!(named("/api/conversations/c1/messages"), Some(Named::Conversation("c1".into())));
        assert_eq!(named("/api/conversations/c1"), Some(Named::Conversation("c1".into())));
        assert_eq!(named("/api/conversations"), None, "the list is scoped by its handler");
        assert_eq!(named("/api/conversations/"), None);
        assert_eq!(named("/api/sessions/c2/action"), Some(Named::Conversation("c2".into())));
        assert_eq!(named("/api/sessions/recovery"), None);
        assert_eq!(named("/api/workspaces/w1/knowledge/clear"), Some(Named::Workspace("w1".into())));
        assert_eq!(named("/api/workspaces"), None);
        assert_eq!(named("/api/memory/m1/share"), Some(Named::Memory("m1".into())));
        assert_eq!(named("/api/agent/runs/r1/events"), Some(Named::Run("r1".into())));
        assert_eq!(named("/api/agent/runs"), None);
        assert_eq!(named("/api/artifacts/a1/file"), Some(Named::Artifact("a1".into())));
        assert_eq!(named("/api/conversations/c%2F1/messages"), Some(Named::Conversation("c/1".into())), "decoded as the router decodes it");
        assert_eq!(named("/api/models/m1"), None);
    }

    /// A folder for a project.
    fn folder(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("companion-owner-{name}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "Ada's notes").unwrap();
        dir.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn one_person_cannot_see_or_touch_anothers_work() {
        use crate::roles::tests::{call, json, person, server, status};
        use axum::http::StatusCode;
        use serde_json::json;
        let (app, state) = server();
        let (_, ada) = person(&state, "ada", &[]).await;
        let (_, bob) = person(&state, "bob", &[]).await;

        // Ada's work: a project, a code conversation in it, a message, memories, a run.
        let ada_folder = folder("ada");
        let project = json(&app, call("POST", "/api/workspaces", &ada, Some(json!({ "name": "Ada's", "path": ada_folder })))).await["id"].as_str().unwrap().to_string();
        let conversation = json(&app, call("POST", "/api/conversations", &ada, Some(json!({ "title": "Ada's plans", "model_id": "", "mode": "code", "workspace": project })))).await["id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(status(&app, call("POST", &format!("/api/conversations/{conversation}/messages"), &ada, Some(json!({ "role": "user", "content": "secret plan" })))).await, StatusCode::OK);
        let memory = json(&app, call("POST", "/api/memory", &ada, Some(json!({ "content": "Ada's salary is private", "scope": "global" })))).await["id"].as_str().unwrap().to_string();
        assert_eq!(status(&app, call("POST", "/api/memory", &ada, Some(json!({ "content": "about the plans", "scope": "conversation", "scope_id": conversation })))).await, StatusCode::OK);
        let run = json(&app, call("POST", "/api/agent/run", &ada, Some(json!({ "conversation_id": conversation, "workspace": ada_folder, "task": "Inspect", "mode": "plan" })))).await["run_id"]
            .as_str()
            .unwrap()
            .to_string();

        // Bob's lists hold none of it.
        for list in ["/api/conversations", "/api/v1/conversations", "/api/workspaces", "/api/v1/workspaces", "/api/agent/runs", "/api/memory"] {
            assert_eq!(json(&app, call("GET", list, &bob, None)).await, json!([]), "{list}");
        }
        let sessions = json(&app, call("GET", "/api/sessions", &bob, None)).await.to_string();
        assert!(!sessions.contains(&conversation), "sessions: {sessions}");

        // Every path that names Ada's things is "not found" to Bob.
        let c = &conversation;
        for (method, uri, body) in [
            ("GET", format!("/api/conversations/{c}"), None),
            ("PATCH", format!("/api/conversations/{c}"), Some(json!({ "title": "mine now" }))),
            ("DELETE", format!("/api/conversations/{c}"), None),
            ("GET", format!("/api/conversations/{c}/messages"), None),
            ("POST", format!("/api/conversations/{c}/messages"), Some(json!({ "role": "user", "content": "hi" }))),
            ("GET", format!("/api/conversations/{c}/attachments"), None),
            ("GET", format!("/api/conversations/{c}/context"), None),
            ("GET", format!("/api/conversations/{c}/timeline"), None),
            ("POST", format!("/api/conversations/{c}/fork"), Some(json!({}))),
            ("POST", format!("/api/conversations/{c}/compact"), Some(json!({}))),
            ("POST", format!("/api/conversations/{c}/prepare"), Some(json!({}))),
            ("PATCH", format!("/api/sessions/{c}"), Some(json!({ "priority": "high" }))),
            ("GET", format!("/api/workspaces/{project}"), None),
            ("DELETE", format!("/api/workspaces/{project}"), None),
            ("GET", format!("/api/workspaces/{project}/knowledge"), None),
            ("DELETE", format!("/api/memory/{memory}"), None),
            ("POST", format!("/api/memory/{memory}/share"), Some(json!({ "target_id": "", "scope": "global" }))),
            ("GET", format!("/api/agent/runs/{run}"), None),
            ("POST", format!("/api/agent/runs/{run}/stop"), Some(json!({}))),
            ("GET", format!("/api/artifacts?conversation_id={c}"), None),
            ("GET", format!("/api/tools/executions?conversation_id={c}"), None),
        ] {
            assert_eq!(status(&app, call(method, &uri, &bob, body)).await, StatusCode::NOT_FOUND, "{method} {uri}");
        }

        // Naming Ada's things in a request body does not work either.
        let bob_conversation = json(&app, call("POST", "/api/conversations", &bob, Some(json!({ "title": "Bob's", "model_id": "" })))).await["id"].as_str().unwrap().to_string();
        assert_eq!(status(&app, call("POST", "/api/chat", &bob, Some(json!({ "conversation_id": c, "message": "hello" })))).await, StatusCode::NOT_FOUND);
        assert_eq!(status(&app, call("POST", "/api/agent/run", &bob, Some(json!({ "conversation_id": c, "workspace": ada_folder, "task": "Inspect" })))).await, StatusCode::NOT_FOUND);
        assert_eq!(status(&app, call("POST", "/api/agent/run", &bob, Some(json!({ "workspace": ada_folder, "task": "Inspect", "mode": "plan" })))).await, StatusCode::FORBIDDEN, "her folder is not his project");
        assert_eq!(status(&app, call("POST", "/api/tools/execute", &bob, Some(json!({ "workspace": ada_folder, "tool": "read_file", "args": { "path": "notes.txt" } })))).await, StatusCode::FORBIDDEN);
        assert_eq!(status(&app, call("POST", "/api/conversations", &bob, Some(json!({ "title": "x", "model_id": "", "mode": "code", "workspace": project })))).await, StatusCode::BAD_REQUEST);
        assert_eq!(status(&app, call("POST", "/api/memory", &bob, Some(json!({ "content": "x", "scope": "conversation", "scope_id": c })))).await, StatusCode::NOT_FOUND);
        assert_eq!(status(&app, call("POST", &format!("/api/conversations/{bob_conversation}/share"), &bob, Some(json!({ "target_id": c })))).await, StatusCode::NOT_FOUND);
        assert_eq!(status(&app, call("PATCH", &format!("/api/conversations/{bob_conversation}"), &bob, Some(json!({ "workspace": project })))).await, StatusCode::NOT_FOUND);

        // Ada's saved memory never reaches Bob's model.
        let bobs_context = state.storage.memory_context(&crate::roles::tests::user_of(&state, "bob").await, &bob_conversation, "").await.unwrap();
        assert_eq!(bobs_context.entries, 0, "{}", bobs_context.text);

        // And Ada's work is as she left it.
        assert_eq!(json(&app, call("GET", "/api/conversations", &ada, None)).await.as_array().unwrap().len(), 1);
        let messages = json(&app, call("GET", &format!("/api/conversations/{c}/messages"), &ada, None)).await;
        assert!(messages.as_array().unwrap().iter().any(|m| m["content"] == "secret plan"));
        assert_eq!(json(&app, call("GET", "/api/memory", &ada, None)).await.as_array().unwrap().len(), 1, "her global memory (the other is for the conversation)");
        assert_eq!(status(&app, call("GET", &format!("/api/agent/runs/{run}"), &ada, None)).await, StatusCode::OK);
        let _ = std::fs::remove_dir_all(&ada_folder);
    }
}
