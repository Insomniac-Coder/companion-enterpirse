//! Server-side generation lifecycle (§46: Stop actually stops).
//!
//! The UI fetch abort alone only closes the SSE reader — the sidecar request
//! would keep burning GPU and its tokens would be lost. The tracker keeps the
//! active generation's cancel flag, partial text, and task handle so
//! `POST /api/chat/stop` (or a superseding new turn) can:
//! 1. flag cancellation (the sidecar stream breaks at the next chunk),
//! 2. abort the task (drops the reqwest stream even if stuck),
//! 3. persist whatever was streamed so far — never silently discard (§83).

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::task::JoinHandle;

pub struct ActiveGeneration {
    pub id: String,
    pub conversation_id: Option<String>,
    /// Whose reply it is: only they stop it.
    pub user_id: String,
    pub cancel: Arc<AtomicBool>,
    pub partial: Arc<Mutex<String>>,
    pub persisted: Arc<AtomicBool>,
    pub handle: JoinHandle<()>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CancelOutcome {
    pub stopped: bool,
    pub id: Option<String>,
    pub chars_kept: usize,
}

/// One reply per conversation (Phase 1, task 7): a new turn supersedes only
/// its own conversation's reply, and Stop stops only one person's. A reply
/// outside any conversation is keyed by its person.
#[derive(Default)]
pub struct GenerationTracker {
    current: std::collections::HashMap<String, ActiveGeneration>,
}

/// Where a reply is tracked: its conversation, or its person's conversation-less slot.
pub fn reply_key(conversation_id: Option<&str>, user_id: &str) -> String {
    match conversation_id.filter(|id| !id.is_empty()) {
        Some(id) => format!("conversation:{id}"),
        None => format!("person:{user_id}"),
    }
}

impl GenerationTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether any reply is running (the model server is busy for someone).
    pub fn is_active(&self) -> bool {
        self.current.values().any(|g| !g.handle.is_finished())
    }

    /// How many replies are running.
    pub fn running(&self) -> usize {
        self.current.values().filter(|g| !g.handle.is_finished()).count()
    }

    /// Conversations with a live reply — ACTIVE residency (§126).
    pub fn active_conversations(&self) -> Vec<String> {
        self.current.values().filter(|g| !g.handle.is_finished()).filter_map(|g| g.conversation_id.clone()).collect()
    }

    /// Track `gen`; the caller has cancelled whatever its conversation had running.
    pub fn insert(&mut self, gen: ActiveGeneration) {
        self.current.insert(reply_key(gen.conversation_id.as_deref(), &gen.user_id), gen);
    }

    /// Remove the tracked generation if it is the given id (completion cleanup).
    pub fn clear_if(&mut self, id: &str) {
        self.current.retain(|_, g| g.id != id);
    }

    /// Stop every reply (Companion closing).
    pub async fn cancel_all(&mut self, storage: &crate::storage::Storage) -> usize {
        let keys: Vec<String> = self.current.keys().cloned().collect();
        let mut stopped = 0;
        for key in keys {
            stopped += usize::from(self.cancel(storage, &key).await.stopped);
        }
        stopped
    }

    /// Cancel the reply tracked at `key` (see `reply_key`): flag + abort +
    /// persist partial exactly once. Safe when there is none (`stopped: false`).
    pub async fn cancel(
        &mut self,
        storage: &crate::storage::Storage,
        key: &str,
    ) -> CancelOutcome {
        let Some(gen) = self.current.remove(key) else {
            return CancelOutcome {
                stopped: false,
                id: None,
                chars_kept: 0,
            };
        };
        gen.cancel.store(true, Ordering::SeqCst);
        gen.handle.abort();
        // Claim the single persistence right; the background task (if still
        // alive) will see `persisted == true` and skip its own save.
        let claimed = gen
            .persisted
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
        let text = gen.partial.lock().expect("lock").clone();
        let mut kept = 0usize;
        if claimed {
            if let (Some(cid), true) = (gen.conversation_id.clone(), !text.is_empty()) {
                let st = storage;
                if st
                    .add_message(&crate::storage::Message {
                        id: gen.id.clone(),
                        conversation_id: cid,
                        role: "assistant".into(),
                        content: text.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                    }).await
                    .is_ok()
                {
                    kept = text.len();
                    if st
                        .message_activities(&gen.id).await
                        .map(|events| !events.is_empty())
                        .unwrap_or(false)
                    {
                        let _ = st.record_message_activity(&gen.id, &crate::agent::AgentEvent::activity(
                            "status", crate::agent::AgentState::Cancelled,
                            "Stopped by you. Completed inspections and the partial response were kept.".into(), 0,
                        )).await;
                    }
                }
            }
        }
        tracing::info!(id = %gen.id, kept, "generation cancelled; partial kept");
        CancelOutcome {
            stopped: true,
            id: Some(gen.id),
            chars_kept: kept,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_persists_partial_and_is_idempotent() {
        let storage = crate::storage::testing::storage();
        storage
            .create_conversation(&crate::storage::Conversation {
                id: "c1".into(),
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
            }).await
            .unwrap();

        let mut tracker = GenerationTracker::new();
        assert!(!tracker.is_active());
        let out = tracker.cancel(&storage, &reply_key(Some("c1"), "local")).await;
        assert!(!out.stopped, "idle cancel must report stopped:false");

        // Fake a stuck generation holding partial text.
        let partial = Arc::new(Mutex::new("Hello wo".to_string()));
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        });
        tracker.insert(ActiveGeneration {
            id: "g1".into(),
            conversation_id: Some("c1".into()),
            user_id: "local".into(),
            cancel: Arc::new(AtomicBool::new(false)),
            partial: partial.clone(),
            persisted: Arc::new(AtomicBool::new(false)),
            handle,
        });
        assert!(tracker.is_active());

        let out = tracker.cancel(&storage, &reply_key(Some("c1"), "local")).await;
        assert!(out.stopped);
        assert_eq!(out.id.as_deref(), Some("g1"));
        assert!(out.chars_kept > 0);
        let msgs = storage.messages_for("c1").await.unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "Hello wo", "persist exactly what streamed");

        // Second cancel finds nothing and persists nothing more.
        let out = tracker.cancel(&storage, &reply_key(Some("c1"), "local")).await;
        assert!(!out.stopped);
        assert_eq!(storage.messages_for("c1").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn cancel_without_conversation_keeps_nothing_but_reports_stopped() {
        let storage = crate::storage::testing::storage();
        let mut tracker = GenerationTracker::new();
        tracker.insert(ActiveGeneration {
            id: "g2".into(),
            conversation_id: None,
            user_id: "local".into(),
            cancel: Arc::new(AtomicBool::new(false)),
            partial: Arc::new(Mutex::new("ephemeral".into())),
            persisted: Arc::new(AtomicBool::new(false)),
            handle: tokio::spawn(async {}),
        });
        let out = tracker.cancel(&storage, &reply_key(None, "local")).await;
        assert!(out.stopped);
        assert_eq!(out.chars_kept, 0);
    }

    #[tokio::test]
    async fn a_reply_is_stopped_alone_not_with_everyone_elses() {
        let storage = crate::storage::testing::storage();
        let mut tracker = GenerationTracker::new();
        let live = |id: &str, conversation: Option<&str>, user: &str| ActiveGeneration {
            id: id.into(),
            conversation_id: conversation.map(String::from),
            user_id: user.into(),
            cancel: Arc::new(AtomicBool::new(false)),
            partial: Arc::new(Mutex::new(String::new())),
            persisted: Arc::new(AtomicBool::new(false)),
            handle: tokio::spawn(async { tokio::time::sleep(std::time::Duration::from_secs(3600)).await }),
        };
        tracker.insert(live("ada-1", Some("ada-conv"), "ada"));
        tracker.insert(live("bob-1", Some("bob-conv"), "bob"));
        tracker.insert(live("bob-2", None, "bob"));
        assert_eq!(tracker.running(), 3);
        assert!(tracker.cancel(&storage, &reply_key(Some("ada-conv"), "ada")).await.stopped);
        assert_eq!(tracker.running(), 2, "Bob's replies carry on");
        let mut live_conversations = tracker.active_conversations();
        live_conversations.sort();
        assert_eq!(live_conversations, ["bob-conv"]);
        assert_eq!(tracker.cancel_all(&storage).await, 2);
        assert!(!tracker.is_active());
    }
}
