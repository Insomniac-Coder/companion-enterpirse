//! Settings in three levels, with locks (Phase 1, task 6).
//!
//! Every field of the settings is one of three kinds:
//! - **personal**: the company sets a default, a group may set its own, and each person may choose
//!   (theme, keyboard, reasoning and sampling defaults, compaction, search consent);
//! - **policy**: the company sets it and a group may set its own, but not a person (allowed and
//!   blocked folders, network rules, file and command limits);
//! - **machine**: the company only, because everyone shares the one model server and its logs
//!   (runtime, hardware, the search provider and its key, logging). A field not listed is machine.
//!
//! A field can be locked for the company (platform admin) or for a group (its team admin): nobody
//! below may change it then. When a person's groups disagree, the group with the lowest priority
//! number wins. The company's settings stay where they always were (`AppState.settings`); a person's
//! effective settings are those with their groups' values and their own choices laid over them.

use crate::api::{check_settings, ApiError, AppState};
use crate::auth::Caller;
use crate::roles::{PLATFORM_ADMIN, TEAM_ADMIN};
use crate::settings::AppSettings;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Personal,
    Policy,
    Machine,
}

const PERSONAL: [&str; 21] = [
    "agent.permission_mode",
    "general.theme",
    "general.default_model",
    "general.language",
    "appearance.",
    "keyboard.",
    "diagnostics.show_generation_speed",
    "diagnostics.show_detailed_metrics",
    "reasoning.default_on",
    "reasoning.budget",
    "memory.auto_compact",
    "memory.compaction_keep_turns",
    "memory.compact_at_pct",
    "memory.share_across_modes",
    "inference.temperature",
    "inference.top_p",
    "inference.top_k",
    "inference.repeat_penalty",
    "search.autonomous",
    "workspace.confirm_outside_copy",
    "workspace.default_dir",
];

const POLICY: [&str; 10] = [
    "agent.max_permission_mode",
    "security.allowed_dirs",
    "security.blocked_dirs",
    "network.policy",
    "network.trusted_hosts",
    "files.max_attach_mb",
    "files.max_image_mb",
    "files.ocr_enabled",
    "agent.command_timeout_secs",
    "privacy.record_model_requests",
];

/// What kind of field `path` is. A prefix ending in "." covers a whole section.
pub fn kind(path: &str) -> Kind {
    let matches = |entry: &&str| if entry.ends_with('.') { path.starts_with(*entry) } else { path == *entry };
    if PERSONAL.iter().any(matches) {
        Kind::Personal
    } else if POLICY.iter().any(matches) {
        Kind::Policy
    } else {
        Kind::Machine
    }
}

/// A folder's path for comparing: as the system resolves it, without Windows' `\\?\` prefix,
/// and in lower case where the file system ignores case.
fn comparable(path: &std::path::Path) -> std::path::PathBuf {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    std::path::PathBuf::from(if cfg!(windows) { text.to_lowercase() } else { text.to_string() })
}

/// Whether a project may be in `folder` under these settings: inside one of the allowed folders
/// when any are listed, and inside none of the blocked ones.
pub fn folder_allowed(settings: &AppSettings, folder: &std::path::Path) -> Result<(), String> {
    let here = comparable(folder);
    let inside = |root: &String| !root.trim().is_empty() && here.starts_with(comparable(std::path::Path::new(root.trim())));
    if let Some(blocked) = settings.security.blocked_dirs.iter().find(|root| inside(root)) {
        return Err(format!("{} is inside a blocked folder ({})", folder.display(), blocked.trim()));
    }
    let allowed: Vec<&str> = settings.security.allowed_dirs.iter().map(|root| root.trim()).filter(|root| !root.is_empty()).collect();
    if !allowed.is_empty() && !settings.security.allowed_dirs.iter().any(|root| inside(root)) {
        return Err(format!("{} is outside the allowed folders ({})", folder.display(), allowed.join(", ")));
    }
    Ok(())
}

/// Every leaf of a settings value with its dotted path (objects are walked; anything else is a leaf).
fn leaves(value: &Value) -> Vec<(String, Value)> {
    fn walk(value: &Value, prefix: &str, out: &mut Vec<(String, Value)>) {
        match value {
            Value::Object(map) => {
                for (key, inner) in map {
                    let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
                    walk(inner, &path, out);
                }
            }
            leaf => out.push((prefix.to_string(), leaf.clone())),
        }
    }
    let mut out = Vec::new();
    walk(value, "", &mut out);
    out
}

/// The leaves of a partial settings value, as changes to lay over the settings.
pub fn leaves_of(value: &Value) -> Vec<(String, Value)> {
    leaves(value)
}

fn set(value: &mut Value, path: &str, leaf: Value) {
    let mut at = value;
    let parts: Vec<&str> = path.split('.').collect();
    for part in &parts[..parts.len() - 1] {
        if !at.get(*part).is_some_and(Value::is_object) {
            at[*part] = Value::Object(Default::default());
        }
        at = at.get_mut(*part).expect("just made");
    }
    at[parts[parts.len() - 1]] = leaf;
}

/// What a person may know about a field: its kind, whether it is locked for them and why.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Field {
    pub kind: Kind,
    /// "company", a group's name, or none.
    pub locked_by: Option<String>,
    pub reason: String,
    /// Whether this person can change it from their own settings.
    pub editable: bool,
}

struct Lock {
    path: String,
    group: Option<String>,
    group_name: String,
    reason: String,
}

/// The locks that bind `user`: the company's and their groups'.
async fn locks_for(pool: &sqlx::PgPool, user: &str) -> Result<Vec<Lock>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT l.path, l.group_id, COALESCE(NULLIF(g.name, ''), g.external_id, '') AS group_name, l.reason FROM setting_locks l
         LEFT JOIN groups g ON g.id = l.group_id
         WHERE l.group_id IS NULL OR l.group_id IN (SELECT group_id FROM group_members WHERE user_id = $1)
         ORDER BY l.group_id NULLS FIRST",
    )
    .bind(user)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| Lock { path: row.get("path"), group: row.get("group_id"), group_name: row.get("group_name"), reason: row.get("reason") })
        .collect())
}

/// The values of `user`'s groups, weakest first (so the strongest is laid down last).
async fn group_values(pool: &sqlx::PgPool, user: &str) -> Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT s.settings FROM group_settings s JOIN group_members m ON m.group_id = s.group_id JOIN groups g ON g.id = s.group_id
         WHERE m.user_id = $1 ORDER BY s.priority DESC, g.name DESC, g.id DESC",
    )
    .bind(user)
    .fetch_all(pool)
    .await
}

async fn own_values(pool: &sqlx::PgPool, user: &str) -> Result<Value, sqlx::Error> {
    Ok(sqlx::query_scalar("SELECT settings FROM user_settings WHERE user_id = $1").bind(user).fetch_optional(pool).await?.unwrap_or(Value::Object(Default::default())))
}

fn locked<'a>(locks: &'a [Lock], path: &str) -> Option<&'a Lock> {
    locks.iter().find(|lock| lock.path == path)
}

/// `user`'s settings: the company's, their groups' values, then their own choices; and what they may
/// know about each field.
pub async fn effective(state: &AppState, user: &str) -> Result<(AppSettings, BTreeMap<String, Field>), sqlx::Error> {
    let company = state.settings.read().await.clone();
    let pool = state.storage.pool();
    let mut value = serde_json::to_value(&company).unwrap_or_default();
    let locks = locks_for(pool, user).await?;
    let company_locked = |path: &str| locks.iter().any(|lock| lock.path == path && lock.group.is_none());
    for group in group_values(pool, user).await? {
        for (path, leaf) in leaves(&group) {
            if kind(&path) != Kind::Machine && !company_locked(&path) {
                set(&mut value, &path, leaf);
            }
        }
    }
    for (path, leaf) in leaves(&own_values(pool, user).await?) {
        if kind(&path) == Kind::Personal && locked(&locks, &path).is_none() {
            set(&mut value, &path, leaf);
        }
    }
    let settings = serde_json::from_value::<AppSettings>(value.clone()).map(AppSettings::normalized).unwrap_or(company);
    let fields = leaves(&serde_json::to_value(&settings).unwrap_or_default())
        .into_iter()
        .map(|(path, _)| {
            let lock = locked(&locks, &path);
            let field = Field {
                kind: kind(&path),
                locked_by: lock.map(|lock| if lock.group.is_some() { lock.group_name.clone() } else { "company".to_string() }),
                reason: lock.map(|lock| lock.reason.clone()).unwrap_or_default(),
                editable: kind(&path) == Kind::Personal && lock.is_none(),
            };
            (path, field)
        })
        .collect();
    Ok((settings, fields))
}

/// The person's settings: effective for a server with sign-in, the company's on a laptop.
pub async fn for_person(state: &AppState, user: &str) -> AppSettings {
    if !state.auth.sign_in_required() {
        return state.settings.read().await.clone();
    }
    match effective(state, user).await {
        Ok((settings, _)) => settings,
        Err(error) => {
            tracing::warn!(%error, "cannot read a person's settings; using the company's");
            state.settings.read().await.clone()
        }
    }
}

/// Where each change from `before` to `after` goes, for `caller`: their own choices, or the
/// company's settings (a platform admin changing a machine or policy field, or the company value of
/// a field the company locked). Anything else is refused, by name.
pub async fn sort_changes(
    state: &AppState,
    caller: &Caller,
    before: &AppSettings,
    after: &AppSettings,
) -> Result<(Vec<(String, Value)>, Vec<(String, Value)>), ApiError> {
    let old: BTreeMap<String, Value> = leaves(&serde_json::to_value(before).unwrap_or_default()).into_iter().collect();
    let changed: Vec<(String, Value)> =
        leaves(&serde_json::to_value(after).unwrap_or_default()).into_iter().filter(|(path, leaf)| old.get(path) != Some(leaf)).collect();
    // A laptop: one person, one set of settings.
    if !state.auth.sign_in_required() {
        return Ok((changed, Vec::new()));
    }
    let locks = locks_for(state.storage.pool(), &caller.id).await.map_err(|e| ApiError::internal(format!("storage error: {e}")))?;
    let admin = caller.has(PLATFORM_ADMIN);
    let (mut company, mut own, mut refused) = (Vec::new(), Vec::new(), Vec::new());
    for (path, leaf) in changed {
        match (kind(&path), locked(&locks, &path)) {
            (Kind::Personal, None) => own.push((path, leaf)),
            (_, Some(lock)) if lock.group.is_some() => refused.push(path),
            _ if admin => company.push((path, leaf)),
            _ => refused.push(path),
        }
    }
    if !refused.is_empty() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            format!("these settings are set for you: {}", refused.join(", ")),
            "They are locked, or the company or your group sets them. Nothing was saved.",
        ));
    }
    Ok((company, own))
}

/// Lay `changes` over `settings`.
pub fn apply(settings: &AppSettings, changes: &[(String, Value)]) -> Result<AppSettings, ApiError> {
    let mut value = serde_json::to_value(settings).unwrap_or_default();
    for (path, leaf) in changes {
        set(&mut value, path, leaf.clone());
    }
    serde_json::from_value::<AppSettings>(value).map(AppSettings::normalized).map_err(|e| ApiError::bad(format!("settings do not fit: {e}"), "Check the values."))
}

/// Merge `changes` into `user`'s own choices.
pub async fn save_own(state: &AppState, user: &str, changes: &[(String, Value)]) -> Result<(), ApiError> {
    if changes.is_empty() {
        return Ok(());
    }
    let pool = state.storage.pool();
    let storage_error = |e: sqlx::Error| ApiError::internal(format!("storage error: {e}"));
    let mut own = own_values(pool, user).await.map_err(storage_error)?;
    for (path, leaf) in changes {
        set(&mut own, path, leaf.clone());
    }
    sqlx::query(
        "INSERT INTO user_settings (user_id, settings) VALUES ($1, $2)
         ON CONFLICT (user_id) DO UPDATE SET settings = EXCLUDED.settings, updated_at = now()",
    )
    .bind(user)
    .bind(&own)
    .execute(pool)
    .await
    .map_err(storage_error)?;
    Ok(())
}

/// The caller's fields: kind, lock and whether they can change it.
pub async fn fields(State(state): State<AppState>, Extension(caller): Extension<Caller>) -> Result<Json<BTreeMap<String, Field>>, ApiError> {
    if !state.auth.sign_in_required() {
        let company = serde_json::to_value(&*state.settings.read().await).unwrap_or_default();
        return Ok(Json(
            leaves(&company)
                .into_iter()
                .map(|(path, _)| (path.clone(), Field { kind: kind(&path), locked_by: None, reason: String::new(), editable: true }))
                .collect(),
        ));
    }
    let (_, mut fields) = effective(&state, &caller.id).await.map_err(|e| ApiError::internal(format!("storage error: {e}")))?;
    // A platform admin's changes to company fields go to the company's settings, a group's lock aside.
    if caller.has(PLATFORM_ADMIN) {
        for field in fields.values_mut() {
            field.editable = field.editable || field.locked_by.as_deref().is_none_or(|by| by == "company");
        }
    }
    Ok(Json(fields))
}

// --- The admin's side: group values and locks. ---

/// May `caller` set values or locks for `group` (None: the company)?
fn may_manage(caller: &Caller, group: Option<&str>) -> Result<(), ApiError> {
    let allowed = caller.has(PLATFORM_ADMIN) || group.is_some_and(|group| caller.has(TEAM_ADMIN) && caller.team_admin_of.iter().any(|g| g == group));
    if allowed {
        Ok(())
    } else {
        Err(ApiError::new(StatusCode::FORBIDDEN, "only a platform admin, or this group's team admin, can do this", "Ask a platform admin of this Companion."))
    }
}

/// The company's values (secrets hidden), every group's, and every lock (platform admins and auditors).
pub async fn overview(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let pool = state.storage.pool();
    let storage_error = |e: sqlx::Error| ApiError::internal(format!("storage error: {e}"));
    let groups = sqlx::query("SELECT s.group_id, g.name, g.external_id, s.settings, s.priority FROM group_settings s JOIN groups g ON g.id = s.group_id ORDER BY s.priority, g.name")
        .fetch_all(pool)
        .await
        .map_err(storage_error)?;
    let locks = sqlx::query("SELECT path, group_id, reason, set_by, set_at::TEXT AS set_at FROM setting_locks ORDER BY group_id NULLS FIRST, path")
        .fetch_all(pool)
        .await
        .map_err(storage_error)?;
    Ok(Json(serde_json::json!({
        "company": crate::api::without_secrets(state.settings.read().await.clone()),
        "groups": groups.iter().map(|row| serde_json::json!({
            "group_id": row.get::<String, _>("group_id"),
            "name": row.get::<String, _>("name"),
            "external_id": row.get::<String, _>("external_id"),
            "settings": row.get::<Value, _>("settings"),
            "priority": row.get::<i32, _>("priority"),
        })).collect::<Vec<_>>(),
        "locks": locks.iter().map(|row| serde_json::json!({
            "path": row.get::<String, _>("path"),
            "group_id": row.get::<Option<String>, _>("group_id"),
            "reason": row.get::<String, _>("reason"),
            "set_by": row.get::<String, _>("set_by"),
            "set_at": row.get::<String, _>("set_at"),
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct GroupValues {
    /// Only the fields the group sets, nested as in the settings.
    settings: Value,
    /// Platform admins only.
    #[serde(default)]
    priority: Option<i32>,
}

/// Set a group's values (replacing the ones it had).
pub async fn put_group(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(group): Path<String>,
    Json(request): Json<GroupValues>,
) -> Result<Json<Value>, ApiError> {
    may_manage(&caller, Some(&group))?;
    if request.priority.is_some() && !caller.has(PLATFORM_ADMIN) {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "only a platform admin sets a group's priority", "Leave priority out."));
    }
    let pool = state.storage.pool();
    let storage_error = |e: sqlx::Error| ApiError::internal(format!("storage error: {e}"));
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM groups WHERE id = $1)").bind(&group).fetch_one(pool).await.map_err(storage_error)?;
    if !exists {
        return Err(ApiError::not_found("no such group"));
    }
    let values = leaves(&request.settings);
    let machine: Vec<&str> = values.iter().filter(|(path, _)| kind(path) == Kind::Machine).map(|(path, _)| path.as_str()).collect();
    if !request.settings.is_object() || !machine.is_empty() {
        return Err(ApiError::bad(format!("a group cannot set these: {}", machine.join(", ")), "Groups set personal and policy fields only; the rest is the company's."));
    }
    // The values must make sense on top of the company's.
    check_settings(&apply(&state.settings.read().await.clone(), &values)?)?;
    sqlx::query(
        "INSERT INTO group_settings (group_id, settings, priority, updated_by) VALUES ($1, $2, COALESCE($3, 100), $4)
         ON CONFLICT (group_id) DO UPDATE SET settings = EXCLUDED.settings, priority = COALESCE($3, group_settings.priority),
         updated_by = EXCLUDED.updated_by, updated_at = now()",
    )
    .bind(&group)
    .bind(&request.settings)
    .bind(request.priority)
    .bind(&caller.id)
    .execute(pool)
    .await
    .map_err(storage_error)?;
    tracing::info!(by = %caller.id, group = %group, "group settings set");
    Ok(Json(serde_json::json!({ "group_id": group, "settings": request.settings })))
}

#[derive(Deserialize)]
pub struct LockRequest {
    path: String,
    #[serde(default)]
    group_id: Option<String>,
    #[serde(default)]
    reason: String,
}

/// Lock a field for the company or for a group.
pub async fn put_lock(State(state): State<AppState>, Extension(caller): Extension<Caller>, Json(request): Json<LockRequest>) -> Result<Json<Value>, ApiError> {
    let group = request.group_id.as_deref().filter(|group| !group.is_empty());
    may_manage(&caller, group)?;
    let known = leaves(&serde_json::to_value(&*state.settings.read().await).unwrap_or_default()).into_iter().any(|(path, _)| path == request.path);
    if !known || kind(&request.path) == Kind::Machine {
        return Err(ApiError::bad(format!("'{}' cannot be locked", request.path), "Lock a personal or policy field (machine fields are the company's anyway)."));
    }
    let reason = request.reason.trim();
    if reason.chars().count() > 300 {
        return Err(ApiError::bad("the reason is too long", "Keep it to 300 characters."));
    }
    sqlx::query(
        "INSERT INTO setting_locks (path, group_id, reason, set_by) VALUES ($1, $2, $3, $4)
         ON CONFLICT (path, COALESCE(group_id, '')) DO UPDATE SET reason = EXCLUDED.reason, set_by = EXCLUDED.set_by, set_at = now()",
    )
    .bind(&request.path)
    .bind(group)
    .bind(reason)
    .bind(&caller.id)
    .execute(state.storage.pool())
    .await
    .map_err(|e| ApiError::internal(format!("storage error: {e}")))?;
    tracing::info!(by = %caller.id, path = %request.path, group = ?group, "setting locked");
    Ok(Json(serde_json::json!({ "locked": request.path, "group_id": group })))
}

/// Unlock a field.
pub async fn delete_lock(State(state): State<AppState>, Extension(caller): Extension<Caller>, Json(request): Json<LockRequest>) -> Result<Json<Value>, ApiError> {
    let group = request.group_id.as_deref().filter(|group| !group.is_empty());
    may_manage(&caller, group)?;
    let done = sqlx::query("DELETE FROM setting_locks WHERE path = $1 AND group_id IS NOT DISTINCT FROM $2")
        .bind(&request.path)
        .bind(group)
        .execute(state.storage.pool())
        .await
        .map_err(|e| ApiError::internal(format!("storage error: {e}")))?;
    if done.rows_affected() == 0 {
        return Err(ApiError::not_found("no such lock"));
    }
    tracing::info!(by = %caller.id, path = %request.path, group = ?group, "setting unlocked");
    Ok(Json(serde_json::json!({ "unlocked": request.path, "group_id": group })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::tests::{call, json, person, server, status, user_of};
    use serde_json::json;

    /// A directory group with `members`, at `priority`, setting `values`.
    async fn group(state: &AppState, name: &str, priority: i32, values: Value, members: &[&str]) -> String {
        let pool = state.storage.pool();
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO groups (id, external_id, name) VALUES ($1, $2, $3)").bind(&id).bind(format!("dir-{name}")).bind(name).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO group_settings (group_id, settings, priority) VALUES ($1, $2, $3)").bind(&id).bind(values).bind(priority).execute(pool).await.unwrap();
        for member in members {
            sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2)").bind(&id).bind(user_of(state, member).await).execute(pool).await.unwrap();
        }
        id
    }

    async fn lock(state: &AppState, path: &str, group: Option<&str>, reason: &str) {
        sqlx::query("INSERT INTO setting_locks (path, group_id, reason) VALUES ($1, $2, $3)").bind(path).bind(group).bind(reason).execute(state.storage.pool()).await.unwrap();
    }

    #[test]
    fn every_field_is_personal_policy_or_machine() {
        assert_eq!(kind("appearance.theme"), Kind::Personal);
        assert_eq!(kind("inference.temperature"), Kind::Personal);
        assert_eq!(kind("security.allowed_dirs"), Kind::Policy);
        assert_eq!(kind("files.max_attach_mb"), Kind::Policy);
        assert_eq!(kind("hardware.gpu_layers"), Kind::Machine);
        assert_eq!(kind("inference.context_size"), Kind::Machine, "one model server for everyone");
        assert_eq!(kind("search.brave_key"), Kind::Machine);
        assert_eq!(kind("something.new"), Kind::Machine, "a field nobody listed is the company's");
        assert_eq!(kind("appearance"), Kind::Machine, "a section prefix is not a field");
    }

    #[tokio::test]
    async fn groups_then_the_person_then_locks_decide() {
        let (_, state) = server();
        person(&state, "ada", &[]).await;
        person(&state, "bob", &[]).await;
        let ada = user_of(&state, "ada").await;
        let bob = user_of(&state, "bob").await;
        let strong = group(&state, "Engineering", 10, json!({ "appearance": { "theme": "light", "density": "compact" }, "files": { "max_attach_mb": 20 } }), &["ada"]).await;
        group(&state, "Everyone", 50, json!({ "appearance": { "theme": "system" } }), &["ada", "bob"]).await;
        let (mine, _) = effective(&state, &ada).await.unwrap();
        assert_eq!((mine.appearance.theme.as_str(), mine.files.max_attach_mb), ("light", 20), "the lower priority number wins");
        assert_eq!(effective(&state, &bob).await.unwrap().0.appearance.theme, "system");

        // Her own choice beats the groups'; policy and machine fields never come from her.
        save_own(&state, &ada, &[("appearance.theme".into(), json!("dark")), ("files.max_attach_mb".into(), json!(50)), ("hardware.gpu_layers".into(), json!(3))]).await.unwrap();
        let (mine, fields) = effective(&state, &ada).await.unwrap();
        assert_eq!(mine.appearance.theme, "dark");
        assert_eq!(mine.files.max_attach_mb, 20);
        assert_eq!(mine.hardware.gpu_layers, state.settings.read().await.hardware.gpu_layers);
        assert!(fields["appearance.theme"].editable && !fields["files.max_attach_mb"].editable);

        // A group lock keeps her choice out; a company lock keeps the groups out too.
        lock(&state, "appearance.density", Some(&strong), "Screens in the lab are small").await;
        save_own(&state, &ada, &[("appearance.density".into(), json!("comfortable"))]).await.unwrap();
        let (mine, fields) = effective(&state, &ada).await.unwrap();
        assert_eq!(mine.appearance.density, "compact");
        assert_eq!((fields["appearance.density"].locked_by.as_deref(), fields["appearance.density"].reason.as_str()), (Some("Engineering"), "Screens in the lab are small"));
        lock(&state, "appearance.theme", None, "Brand colours").await;
        let company_theme = state.settings.read().await.appearance.theme.clone();
        assert_eq!(effective(&state, &ada).await.unwrap().0.appearance.theme, company_theme);
        assert_eq!(effective(&state, &bob).await.unwrap().0.appearance.theme, company_theme);
        assert_eq!(effective(&state, &bob).await.unwrap().1["appearance.theme"].locked_by.as_deref(), Some("company"));
    }

    #[tokio::test]
    async fn a_person_saves_their_own_choices_and_only_those() {
        let (app, state) = server();
        let (_, ada) = person(&state, "ada", &[]).await;
        let (_, bob) = person(&state, "bob", &[]).await;
        let (_, boss) = person(&state, "boss", &[PLATFORM_ADMIN]).await;
        let shown = json(&app, call("GET", "/api/settings", &ada, None)).await;

        let mut light = shown.clone();
        light["appearance"]["theme"] = json!("light");
        assert_eq!(status(&app, call("PUT", "/api/settings", &ada, Some(light))).await, StatusCode::OK);
        assert_eq!(json(&app, call("GET", "/api/settings", &ada, None)).await["appearance"]["theme"], "light");
        assert_eq!(json(&app, call("GET", "/api/settings", &bob, None)).await["appearance"]["theme"], shown["appearance"]["theme"], "Bob's is his own");

        let mut bigger = shown.clone();
        bigger["files"]["max_attach_mb"] = json!(40);
        let refused = app.clone().oneshot(call("PUT", "/api/settings", &ada, Some(bigger.clone()))).await.unwrap();
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(refused.into_body(), 100_000).await.unwrap()).unwrap();
        assert!(body["error"].as_str().unwrap().contains("files.max_attach_mb"), "{body}");

        // A platform admin changes the company's value, for everyone.
        assert_eq!(status(&app, call("PUT", "/api/settings", &boss, Some(bigger))).await, StatusCode::OK);
        assert_eq!(json(&app, call("GET", "/api/settings", &bob, None)).await["files"]["max_attach_mb"], 40);
        // And sets a company default for a personal field through the company endpoint.
        assert_eq!(status(&app, call("PUT", "/api/admin/settings/company", &boss, Some(json!({ "settings": { "reasoning": { "default_on": true } } })))).await, StatusCode::OK);
        assert_eq!(json(&app, call("GET", "/api/settings", &bob, None)).await["reasoning"]["default_on"], true);
        assert_eq!(status(&app, call("PUT", "/api/admin/settings/company", &ada, Some(json!({ "settings": {} })))).await, StatusCode::FORBIDDEN);

        let fields = json(&app, call("GET", "/api/settings/fields", &ada, None)).await;
        assert_eq!(fields["appearance.theme"]["kind"], "personal");
        assert_eq!(fields["hardware.gpu_layers"]["editable"], false);
    }

    use axum::http::StatusCode;
    use tower::ServiceExt as _;

    #[tokio::test]
    async fn team_admins_set_their_groups_values_and_locks_only() {
        let (app, state) = server();
        let (tina_id, tina) = person(&state, "tina", &[]).await;
        let mine = group(&state, "Support", 100, json!({}), &["tina"]).await;
        let other = group(&state, "Sales", 100, json!({}), &[]).await;
        sqlx::query("INSERT INTO role_grants (user_id, role, group_id, source) VALUES ($1, 'team_admin', $2, 'admin')").bind(&tina_id).bind(&mine).execute(state.storage.pool()).await.unwrap();

        let values = |settings: Value| Some(json!({ "settings": settings }));
        assert_eq!(status(&app, call("PUT", &format!("/api/admin/settings/groups/{mine}"), &tina, values(json!({ "appearance": { "theme": "light" } })))).await, StatusCode::OK);
        assert_eq!(status(&app, call("PUT", &format!("/api/admin/settings/groups/{other}"), &tina, values(json!({})))).await, StatusCode::FORBIDDEN);
        assert_eq!(status(&app, call("PUT", &format!("/api/admin/settings/groups/{mine}"), &tina, Some(json!({ "settings": {}, "priority": 1 })))).await, StatusCode::FORBIDDEN, "priority is a platform admin's");
        assert_eq!(status(&app, call("PUT", &format!("/api/admin/settings/groups/{mine}"), &tina, values(json!({ "hardware": { "gpu_layers": 2 } })))).await, StatusCode::BAD_REQUEST, "machine fields are the company's");
        assert_eq!(status(&app, call("PUT", &format!("/api/admin/settings/groups/{mine}"), &tina, values(json!({ "files": { "max_attach_mb": 999 } })))).await, StatusCode::BAD_REQUEST, "values are checked");
        let lock = |group: Option<&str>, path: &str| Some(json!({ "path": path, "group_id": group, "reason": "r" }));
        assert_eq!(status(&app, call("PUT", "/api/admin/settings/locks", &tina, lock(Some(&mine), "appearance.theme"))).await, StatusCode::OK);
        assert_eq!(status(&app, call("PUT", "/api/admin/settings/locks", &tina, lock(None, "appearance.theme"))).await, StatusCode::FORBIDDEN, "a company lock is a platform admin's");
        assert_eq!(status(&app, call("PUT", "/api/admin/settings/locks", &tina, lock(Some(&mine), "hardware.gpu_layers"))).await, StatusCode::BAD_REQUEST);
        assert_eq!(status(&app, call("DELETE", "/api/admin/settings/locks", &tina, lock(Some(&mine), "appearance.theme"))).await, StatusCode::OK);
        assert_eq!(status(&app, call("DELETE", "/api/admin/settings/locks", &tina, lock(Some(&mine), "appearance.theme"))).await, StatusCode::NOT_FOUND);
    }

    #[test]
    fn folders_outside_the_rules_are_refused() {
        let base = std::env::temp_dir().join(format!("companion-folders-{}", uuid::Uuid::new_v4().simple()));
        let (projects, secret) = (base.join("Projects"), base.join("Projects").join("Secret"));
        std::fs::create_dir_all(&secret).unwrap();
        let mut settings = AppSettings::default();
        assert!(folder_allowed(&settings, &secret).is_ok(), "no rules: anywhere");
        settings.security.allowed_dirs = vec![projects.to_string_lossy().into_owned()];
        assert!(folder_allowed(&settings, &secret).is_ok());
        assert!(folder_allowed(&settings, &base).unwrap_err().contains("outside the allowed folders"));
        settings.security.blocked_dirs = vec![secret.to_string_lossy().into_owned()];
        assert!(folder_allowed(&settings, &secret).unwrap_err().contains("blocked"));
        if cfg!(windows) {
            settings.security.blocked_dirs = vec![secret.to_string_lossy().to_uppercase()];
            assert!(folder_allowed(&settings, &secret).is_err(), "Windows folders ignore case");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn projects_only_where_the_rules_allow_even_after_they_change() {
        let (app, state) = server();
        let (_, ada) = person(&state, "ada", &[]).await;
        let base = std::env::temp_dir().join(format!("companion-rules-{}", uuid::Uuid::new_v4().simple()));
        let (allowed, outside) = (base.join("allowed"), base.join("outside"));
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(allowed.join("a.txt"), "x").unwrap();
        let rules = group(&state, "Locked down", 10, json!({ "security": { "allowed_dirs": [allowed.to_string_lossy()] } }), &["ada"]).await;
        let project = |path: &std::path::Path| call("POST", "/api/workspaces", &ada, Some(json!({ "name": "p", "path": path.to_string_lossy() })));
        assert_eq!(status(&app, project(&outside)).await, StatusCode::FORBIDDEN);
        assert_eq!(status(&app, project(&allowed)).await, StatusCode::OK);
        let read = || call("POST", "/api/tools/execute", &ada, Some(json!({ "workspace": allowed.to_string_lossy(), "tool": "read_file", "args": { "path": "a.txt" } })));
        assert_eq!(status(&app, read()).await, StatusCode::OK);
        sqlx::query("UPDATE group_settings SET settings = $1 WHERE group_id = $2")
            .bind(json!({ "security": { "blocked_dirs": [allowed.to_string_lossy()] } }))
            .bind(&rules)
            .execute(state.storage.pool())
            .await
            .unwrap();
        assert_eq!(status(&app, read()).await, StatusCode::FORBIDDEN, "a project saved before the folder was blocked");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_network_rule_and_the_command_limit_apply() {
        use crate::search::{host_allowed, SearchConfig};
        let mut cfg = SearchConfig::default();
        assert!(host_allowed(&cfg, "html.duckduckgo.com").is_ok(), "ask: as before");
        cfg.network_policy = "disabled".into();
        assert!(host_allowed(&cfg, "html.duckduckgo.com").is_err());
        cfg.network_policy = "selected".into();
        cfg.trusted_hosts = vec!["duckduckgo.com".into()];
        assert!(host_allowed(&cfg, "html.duckduckgo.com").is_ok(), "a host covers its subdomains");
        assert!(host_allowed(&cfg, "api.search.brave.com").is_err());
        assert!(host_allowed(&cfg, "evilduckduckgo.com").is_err());

        let command = |args: Value| crate::tools::ToolRequest { name: "execute_command".into(), args, approved: true };
        let capped = crate::tools::within_time_limit(command(json!({ "command": "make", "timeout_secs": 900 })), 300);
        assert_eq!(capped.args["timeout_secs"], 300);
        let defaulted = crate::tools::within_time_limit(command(json!({ "command": "make" })), 300);
        assert_eq!(defaulted.args["timeout_secs"], crate::terminal::DEFAULT_TIMEOUT_SECS.min(300));
        let read = crate::tools::within_time_limit(crate::tools::ToolRequest { name: "read_file".into(), args: json!({ "path": "a" }), approved: true }, 1);
        assert!(read.args.get("timeout_secs").is_none());
    }

    #[test]
    fn secrets_are_masked_in_the_log() {
        use crate::logfile::masked;
        assert_eq!(masked("Authorization: Bearer abc.def-123"), "Authorization: Bearer ***");
        assert_eq!(masked("key cmp_0123456789abcdef0123 used"), "key cmp_*** used");
        assert_eq!(masked("postgres://companion:hunter2@127.0.0.1:5432/companion"), "postgres://companion:***@127.0.0.1:5432/companion");
        assert_eq!(masked("password=hunter2 user=ada"), "password=*** user=ada");
        assert_eq!(masked(r#"{"client_secret": "s3cr3t"}"#), r#"{"client_secret": "***"}"#);
        assert_eq!(masked(r#"{"brave_key":"BSA123"}"#), r#"{"brave_key":"***"}"#);
        assert_eq!(masked("nothing secret here"), "nothing secret here");
    }

    #[test]
    fn old_defaults_nobody_applied_are_read_as_today_once() {
        let mut old = serde_json::to_value(AppSettings::default()).unwrap();
        old.as_object_mut().unwrap().remove("version");
        old["network"]["policy"] = json!("disabled");
        old["agent"]["command_timeout_secs"] = json!(120);
        let read: AppSettings = serde_json::from_value(old).unwrap();
        let read = read.normalized();
        assert_eq!((read.network.policy.as_str(), read.agent.command_timeout_secs, read.version), ("ask", 1800, 1));
        // A choice made since stays a choice.
        let mut chosen = read.clone();
        chosen.network.policy = "disabled".into();
        chosen.agent.command_timeout_secs = 120;
        let chosen = chosen.normalized();
        assert_eq!((chosen.network.policy.as_str(), chosen.agent.command_timeout_secs), ("disabled", 120));
    }

    #[tokio::test]
    async fn attachments_follow_the_limit_a_person_has() {
        let (app, state) = server();
        let (_, ada) = person(&state, "ada", &[]).await;
        let (_, bob) = person(&state, "bob", &[]).await;
        group(&state, "Small", 10, json!({ "files": { "max_attach_mb": 2 } }), &["bob"]).await;
        let attach = |cookie: &str, conversation: &str, megabytes: usize| {
            call("POST", &format!("/api/conversations/{conversation}/attachments"), cookie, Some(json!({ "filename": "notes.txt", "content": "a".repeat(megabytes * 1_000_000) })))
        };
        let ada_conversation = json(&app, call("POST", "/api/conversations", &ada, Some(json!({ "title": "A", "model_id": "" })))).await["id"].as_str().unwrap().to_string();
        let bob_conversation = json(&app, call("POST", "/api/conversations", &bob, Some(json!({ "title": "B", "model_id": "" })))).await["id"].as_str().unwrap().to_string();
        assert_eq!(status(&app, attach(&ada, &ada_conversation, 3)).await, StatusCode::OK, "3 MB under the company's 5 (it used to stop at the request limit)");
        assert_eq!(status(&app, attach(&ada, &ada_conversation, 6)).await, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(status(&app, attach(&bob, &bob_conversation, 3)).await, StatusCode::PAYLOAD_TOO_LARGE, "his group allows 2");
    }
}
