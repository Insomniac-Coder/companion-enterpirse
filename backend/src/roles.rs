//! Users, groups and roles (Phase 1, task 4).
//!
//! Every signed-in person is a user. On top: a **platform admin** runs the server for everyone (models,
//! downloads, company settings, system checks); a **team admin** looks after one group (its settings
//! arrive with task 6); an **auditor** reads the people, groups and, with task 8, the audit records.
//!
//! Groups come from the company directory: the ID token's `groups` claim (Entra ID sends group object
//! ids), refreshed at each sign-in. Roles come from three places: the token's `roles` claim (Entra app
//! roles with the values `platform_admin` or `auditor`), `COMPANION_PLATFORM_ADMINS` (email addresses,
//! to give a new server its first admin), and grants a platform admin makes here. The first two are
//! renewed at each sign-in; team admin, being for one group, is only granted here.

use crate::api::{ApiError, AppState};
use crate::auth::Caller;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::Deserialize;
use sqlx::Row;

pub const PLATFORM_ADMIN: &str = "platform_admin";
pub const TEAM_ADMIN: &str = "team_admin";
pub const AUDITOR: &str = "auditor";
/// The roles the directory may grant through the `roles` claim.
const FROM_DIRECTORY: [&str; 2] = [PLATFORM_ADMIN, AUDITOR];

/// A person's roles, and the groups they are team admin of.
pub async fn grants(pool: &sqlx::PgPool, user_id: &str) -> Result<(Vec<String>, Vec<String>), sqlx::Error> {
    let rows = sqlx::query("SELECT DISTINCT role, group_id FROM role_grants WHERE user_id = $1 ORDER BY role, group_id")
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    let mut roles: Vec<String> = rows.iter().map(|row| row.get::<String, _>("role")).collect();
    roles.dedup();
    let teams = rows.iter().filter_map(|row| row.get::<Option<String>, _>("group_id")).collect();
    Ok((roles, teams))
}

/// What a sign-in says about groups and roles, from the ID token's claims (already verified).
pub struct Directory {
    /// None when the token says it left the groups out (Entra ID's "overage", over 200 groups):
    /// memberships are then kept as they were.
    pub groups: Option<Vec<String>>,
    pub roles: Vec<String>,
}

impl Directory {
    pub fn from_claims(claims: &serde_json::Value) -> Directory {
        let strings = |value: &serde_json::Value| -> Vec<String> {
            value.as_array().map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect()).unwrap_or_default()
        };
        let overage = claims["_claim_names"]["groups"].is_string() || claims["hasgroups"].as_bool() == Some(true);
        Directory {
            groups: (!overage).then(|| strings(&claims["groups"])),
            roles: strings(&claims["roles"]).into_iter().filter(|role| FROM_DIRECTORY.contains(&role.as_str())).collect(),
        }
    }
}

/// Renew what the directory and the configuration say about this person, as part of signing in.
pub async fn renew_at_sign_in(
    tx: &mut sqlx::PgConnection,
    user_id: &str,
    email: &str,
    directory: &Directory,
    platform_admins: &[String],
) -> Result<(), sqlx::Error> {
    match &directory.groups {
        Some(groups) => {
            sqlx::query("DELETE FROM group_members WHERE user_id = $1").bind(user_id).execute(&mut *tx).await?;
            for external_id in groups {
                let group_id: String = sqlx::query_scalar(
                    "INSERT INTO groups (id, external_id) VALUES ($1, $2)
                     ON CONFLICT (external_id) DO UPDATE SET external_id = EXCLUDED.external_id RETURNING id",
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(external_id)
                .fetch_one(&mut *tx)
                .await?;
                sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
                    .bind(&group_id)
                    .bind(user_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        None => tracing::warn!(user = %user_id, "the ID token left the groups out (too many); group memberships were kept as they were"),
    }
    sqlx::query("DELETE FROM role_grants WHERE user_id = $1 AND source IN ('directory', 'config')").bind(user_id).execute(&mut *tx).await?;
    let mut renewed: Vec<(&str, &str)> = directory.roles.iter().map(|role| (role.as_str(), "directory")).collect();
    if !email.is_empty() && platform_admins.iter().any(|admin| admin.eq_ignore_ascii_case(email)) {
        renewed.push((PLATFORM_ADMIN, "config"));
    }
    for (role, source) in renewed {
        sqlx::query("INSERT INTO role_grants (user_id, role, source, granted_by) VALUES ($1, $2, $3, 'sign-in') ON CONFLICT DO NOTHING")
            .bind(user_id)
            .bind(role)
            .bind(source)
            .execute(&mut *tx)
            .await?;
    }
    Ok(())
}

fn forbidden(what: &str) -> Response {
    ApiError::new(StatusCode::FORBIDDEN, format!("only {what} can do this"), "Ask a platform admin of this Companion.").into_response()
}

/// Guards a handler that changes the server for everyone.
pub async fn platform_admin_only(request: Request, next: Next) -> Response {
    match request.extensions().get::<Caller>() {
        Some(caller) if caller.has(PLATFORM_ADMIN) => next.run(request).await,
        _ => forbidden("a platform admin"),
    }
}

/// Guards a read-only view of people and groups.
pub async fn admin_or_auditor(request: Request, next: Next) -> Response {
    match request.extensions().get::<Caller>() {
        Some(caller) if caller.has(PLATFORM_ADMIN) || caller.has(AUDITOR) => next.run(request).await,
        _ => forbidden("a platform admin or an auditor"),
    }
}

fn storage_error(error: sqlx::Error) -> ApiError {
    ApiError::internal(format!("storage error: {error}"))
}

/// The groups `user_id` belongs to, as `{id, name, external_id}`.
pub async fn groups_of(pool: &sqlx::PgPool, user_id: &str) -> Result<Vec<serde_json::Value>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT g.id, g.name, g.external_id FROM group_members m JOIN groups g ON g.id = m.group_id
         WHERE m.user_id = $1 ORDER BY g.name, g.external_id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| serde_json::json!({ "id": row.get::<String, _>("id"), "name": row.get::<String, _>("name"), "external_id": row.get::<String, _>("external_id") }))
        .collect())
}

/// The local person of an install without sign-in exists in every database, but on a server with
/// sign-in nobody can be them: they are left out of the people there.
fn hidden_local(state: &AppState) -> &'static str {
    if state.auth.sign_in_required() { "local" } else { "" }
}

/// Everyone who has signed in, with their roles and groups.
pub async fn list_users(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let pool = state.storage.pool();
    let users = sqlx::query("SELECT id, name, email, last_seen_at::TEXT AS last_seen_at FROM users WHERE id <> $1 ORDER BY name, email")
        .bind(hidden_local(&state))
        .fetch_all(pool)
        .await
        .map_err(storage_error)?;
    let mut out = Vec::with_capacity(users.len());
    for user in &users {
        let id: String = user.get("id");
        let roles = sqlx::query(
            "SELECT r.role, r.group_id, COALESCE(g.name, '') AS group_name, r.source, r.granted_by, r.granted_at::TEXT AS granted_at
             FROM role_grants r LEFT JOIN groups g ON g.id = r.group_id WHERE r.user_id = $1 ORDER BY r.role, r.source",
        )
        .bind(&id)
        .fetch_all(pool)
        .await
        .map_err(storage_error)?;
        out.push(serde_json::json!({
            "id": id,
            "name": user.get::<String, _>("name"),
            "email": user.get::<String, _>("email"),
            "last_seen_at": user.get::<String, _>("last_seen_at"),
            "roles": roles.iter().map(|row| serde_json::json!({
                "role": row.get::<String, _>("role"),
                "group_id": row.get::<Option<String>, _>("group_id"),
                "group_name": row.get::<String, _>("group_name"),
                "source": row.get::<String, _>("source"),
                "granted_by": row.get::<String, _>("granted_by"),
                "granted_at": row.get::<String, _>("granted_at"),
            })).collect::<Vec<_>>(),
            "groups": groups_of(pool, &id).await.map_err(storage_error)?,
        }));
    }
    Ok(Json(serde_json::Value::Array(out)))
}

/// Every group the directory has named, with how many people are in it.
pub async fn list_groups(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT g.id, g.external_id, g.name, count(m.user_id) AS members FROM groups g
         LEFT JOIN group_members m ON m.group_id = g.id GROUP BY g.id ORDER BY g.name, g.external_id",
    )
    .fetch_all(state.storage.pool())
    .await
    .map_err(storage_error)?;
    Ok(Json(serde_json::Value::Array(
        rows.iter()
            .map(|row| serde_json::json!({
                "id": row.get::<String, _>("id"),
                "external_id": row.get::<String, _>("external_id"),
                "name": row.get::<String, _>("name"),
                "members": row.get::<i64, _>("members"),
            }))
            .collect(),
    )))
}

#[derive(Deserialize)]
pub struct GroupName {
    name: String,
}

/// Give a directory group a name people recognise.
pub async fn name_group(State(state): State<AppState>, Path(id): Path<String>, Json(request): Json<GroupName>) -> Result<Json<serde_json::Value>, ApiError> {
    let name = request.name.trim();
    if name.chars().count() > 120 {
        return Err(ApiError::bad("the name is too long", "Keep a group's name to 120 characters."));
    }
    let done = sqlx::query("UPDATE groups SET name = $1 WHERE id = $2").bind(name).bind(&id).execute(state.storage.pool()).await.map_err(storage_error)?;
    if done.rows_affected() == 0 {
        return Err(ApiError::not_found("no such group"));
    }
    Ok(Json(serde_json::json!({ "id": id, "name": name })))
}

#[derive(Deserialize)]
pub struct RoleRequest {
    role: String,
    /// The group, for team admin.
    #[serde(default)]
    group_id: Option<String>,
}

fn check_role(request: &RoleRequest) -> Result<Option<&str>, ApiError> {
    let group = request.group_id.as_deref().filter(|group| !group.is_empty());
    match (request.role.as_str(), group) {
        (PLATFORM_ADMIN | AUDITOR, None) => Ok(None),
        (TEAM_ADMIN, Some(group)) => Ok(Some(group)),
        (TEAM_ADMIN, None) => Err(ApiError::bad("a team admin looks after a group", "Name the group (group_id).")),
        (PLATFORM_ADMIN | AUDITOR, Some(_)) => Err(ApiError::bad("this role is not for one group", "Leave group_id out.")),
        (other, _) => Err(ApiError::bad(format!("unknown role '{other}'"), "Roles are platform_admin, team_admin and auditor.")),
    }
}

/// Grant a role here (it stays until withdrawn here).
pub async fn grant_role(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(user_id): Path<String>,
    Json(request): Json<RoleRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let group = check_role(&request)?;
    let pool = state.storage.pool();
    let known: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE id = $1 AND id <> $2)")
        .bind(&user_id)
        .bind(hidden_local(&state))
        .fetch_one(pool)
        .await
        .map_err(storage_error)?;
    if !known {
        return Err(ApiError::not_found("no such person (they appear here after their first sign-in)"));
    }
    if let Some(group) = group {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM groups WHERE id = $1)").bind(group).fetch_one(pool).await.map_err(storage_error)?;
        if !exists {
            return Err(ApiError::not_found("no such group"));
        }
    }
    sqlx::query("INSERT INTO role_grants (user_id, role, group_id, source, granted_by) VALUES ($1, $2, $3, 'admin', $4) ON CONFLICT DO NOTHING")
        .bind(&user_id)
        .bind(&request.role)
        .bind(group)
        .bind(&caller.id)
        .execute(pool)
        .await
        .map_err(storage_error)?;
    tracing::info!(by = %caller.id, user = %user_id, role = %request.role, group = ?group, "role granted");
    Ok(Json(serde_json::json!({ "granted": request.role, "user": user_id, "group_id": group })))
}

/// Withdraw a role granted here. Roles from the directory or the configuration are changed there.
pub async fn revoke_role(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(user_id): Path<String>,
    Json(request): Json<RoleRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let group = check_role(&request)?;
    if user_id == caller.id && request.role == PLATFORM_ADMIN {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "you cannot withdraw your own platform admin role",
            "Another platform admin can, so the server is never left without one by accident.",
        ));
    }
    let pool = state.storage.pool();
    let done = sqlx::query("DELETE FROM role_grants WHERE user_id = $1 AND role = $2 AND group_id IS NOT DISTINCT FROM $3 AND source = 'admin'")
        .bind(&user_id)
        .bind(&request.role)
        .bind(group)
        .execute(pool)
        .await
        .map_err(storage_error)?;
    if done.rows_affected() == 0 {
        let elsewhere: Option<String> = sqlx::query_scalar(
            "SELECT source FROM role_grants WHERE user_id = $1 AND role = $2 AND group_id IS NOT DISTINCT FROM $3 LIMIT 1",
        )
        .bind(&user_id)
        .bind(&request.role)
        .bind(group)
        .fetch_optional(pool)
        .await
        .map_err(storage_error)?;
        return Err(match elsewhere.as_deref() {
            Some("directory") => ApiError::new(StatusCode::CONFLICT, "this role comes from the company directory", "Remove the app role assignment in the directory; it ends at their next sign-in."),
            Some(_) => ApiError::new(StatusCode::CONFLICT, "this role comes from COMPANION_PLATFORM_ADMINS", "Take them off that list and restart; it ends at their next sign-in."),
            None => ApiError::not_found("they do not have this role"),
        });
    }
    tracing::info!(by = %caller.id, user = %user_id, role = %request.role, group = ?group, "role withdrawn");
    Ok(Json(serde_json::json!({ "withdrawn": request.role, "user": user_id, "group_id": group })))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use sha2::{Digest, Sha256};
    use tower::ServiceExt as _;

    /// Every route that changes the server for everyone, or shows everyone's activity.
    const PLATFORM_ADMIN_ROUTES: [(&str, &str); 29] = [
        ("POST", "/api/models/load"),
        ("POST", "/api/models/unload"),
        ("POST", "/api/models/scan"),
        ("POST", "/api/models/m/tooling/check"),
        ("GET", "/api/models/downloads"),
        ("POST", "/api/models/downloads"),
        ("GET", "/api/models/downloads/d"),
        ("POST", "/api/models/downloads/d/pause"),
        ("POST", "/api/models/downloads/d/resume"),
        ("POST", "/api/models/downloads/d/cancel"),
        ("DELETE", "/api/models/m"),
        ("PUT", "/api/settings"),
        ("PUT", "/api/permissions/mode"),
        ("GET", "/api/system/overview"),
        ("GET", "/api/v1/system/overview"),
        ("POST", "/api/inference/start"),
        ("POST", "/api/inference/stop"),
        ("POST", "/api/models/load/cancel"),
        ("GET", "/api/logs/archive"),
        ("POST", "/api/system/calibrate"),
        ("POST", "/api/models/m/calibrate"),
        ("POST", "/api/plugins/p/run"),
        ("GET", "/api/doctor"),
        ("GET", "/api/setup/status"),
        ("POST", "/api/system/benchmark"),
        ("PATCH", "/api/admin/groups/g"),
        ("POST", "/api/admin/users/u/roles"),
        ("DELETE", "/api/admin/users/u/roles"),
        ("GET", "/api/admin/users"),
    ];

    /// A server with sign-in. Nobody signs in through a provider here: people and their sessions
    /// are written straight into the database.
    pub(crate) fn server() -> (axum::Router, AppState) {
        let auth = crate::auth::Auth {
            open_id: Some(crate::auth::OpenId {
                issuer: "https://login.example.com/v2.0".into(),
                client_id: "companion".into(),
                client_secret: "secret".into(),
                public_url: reqwest::Url::parse("https://companion.example.com").unwrap(),
            }),
            platform_admins: Vec::new(),
        };
        let state = AppState::new_stub().with_auth(auth);
        (crate::api::router(state.clone()), state)
    }

    /// A signed-in person with `roles` granted here; returns their id and session cookie.
    pub(crate) async fn person(state: &AppState, name: &str, roles: &[&str]) -> (String, String) {
        let pool = state.storage.pool();
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO users (id, issuer, subject, email, name) VALUES ($1, 'https://login.example.com/v2.0', $2, $3, $4)")
            .bind(&id)
            .bind(name)
            .bind(format!("{name}@example.com"))
            .bind(name)
            .execute(pool)
            .await
            .unwrap();
        for role in roles {
            sqlx::query("INSERT INTO role_grants (user_id, role, source) VALUES ($1, $2, 'admin')").bind(&id).bind(role).execute(pool).await.unwrap();
        }
        let token = uuid::Uuid::new_v4().simple().to_string();
        sqlx::query("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, $2, now() + interval '1 hour')")
            .bind(format!("{:x}", Sha256::digest(token.as_bytes())))
            .bind(&id)
            .execute(pool)
            .await
            .unwrap();
        (id, format!("companion_session={token}"))
    }

    /// The id of the person named `name` (made with `person`).
    pub(crate) async fn user_of(state: &AppState, name: &str) -> String {
        sqlx::query_scalar("SELECT id FROM users WHERE subject = $1").bind(name).fetch_one(state.storage.pool()).await.unwrap()
    }

    pub(crate) fn call(method: &str, uri: &str, cookie: &str, body: Option<serde_json::Value>) -> Request<Body> {
        let request = Request::builder().method(method).uri(uri).header("cookie", cookie);
        match body {
            Some(body) => request.header("content-type", "application/json").body(Body::from(body.to_string())).unwrap(),
            None => request.body(Body::empty()).unwrap(),
        }
    }

    pub(crate) async fn status(app: &axum::Router, request: Request<Body>) -> StatusCode {
        app.clone().oneshot(request).await.unwrap().status()
    }

    pub(crate) async fn json(app: &axum::Router, request: Request<Body>) -> serde_json::Value {
        let response = app.clone().oneshot(request).await.unwrap();
        serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap()
    }

    #[tokio::test]
    async fn a_user_reaches_none_of_the_admin_routes() {
        let (app, state) = server();
        let (_, user) = person(&state, "ursula", &[]).await;
        let (_, team) = person(&state, "tom", &[AUDITOR]).await;
        for (method, uri) in PLATFORM_ADMIN_ROUTES {
            assert_eq!(status(&app, call(method, uri, &user, Some(serde_json::json!({})))).await, StatusCode::FORBIDDEN, "{method} {uri} for a user");
            let auditor = status(&app, call(method, uri, &team, Some(serde_json::json!({})))).await;
            if uri == "/api/admin/users" {
                assert_eq!(auditor, StatusCode::OK, "an auditor reads people");
            } else {
                assert_eq!(auditor, StatusCode::FORBIDDEN, "{method} {uri} for an auditor");
            }
        }
        // Their own work stays open to them.
        for uri in ["/api/conversations", "/api/models", "/api/settings", "/api/me", "/api/workspaces"] {
            assert_eq!(status(&app, call("GET", uri, &user, None)).await, StatusCode::OK, "{uri}");
        }
    }

    #[tokio::test]
    async fn a_platform_admin_passes_and_roles_change_at_once() {
        let (app, state) = server();
        let (admin, boss) = person(&state, "ada", &[PLATFORM_ADMIN]).await;
        let (user, worker) = person(&state, "bob", &[]).await;
        assert_eq!(status(&app, call("GET", "/api/models/downloads", &boss, None)).await, StatusCode::OK);
        let people = json(&app, call("GET", "/api/admin/users", &boss, None)).await;
        assert!(people.as_array().unwrap().iter().any(|p| p["name"] == "bob" && p["roles"] == serde_json::json!([])));
        assert!(people.as_array().unwrap().iter().all(|p| p["id"] != "local"), "nobody can be the local person here");
        assert_eq!(status(&app, call("POST", "/api/admin/users/local/roles", &boss, Some(serde_json::json!({ "role": AUDITOR })))).await, StatusCode::NOT_FOUND);

        let grant = |role: &str, group: Option<&str>| serde_json::json!({ "role": role, "group_id": group });
        let roles = format!("/api/admin/users/{user}/roles");
        assert_eq!(status(&app, call("POST", &roles, &boss, Some(grant(AUDITOR, None)))).await, StatusCode::OK);
        assert_eq!(status(&app, call("GET", "/api/admin/users", &worker, None)).await, StatusCode::OK, "granted: the next request already has it");
        assert_eq!(status(&app, call("DELETE", &roles, &boss, Some(grant(AUDITOR, None)))).await, StatusCode::OK);
        assert_eq!(status(&app, call("GET", "/api/admin/users", &worker, None)).await, StatusCode::FORBIDDEN, "withdrawn at once");

        // A team admin looks after one group, which must exist.
        assert_eq!(status(&app, call("POST", &roles, &boss, Some(grant(TEAM_ADMIN, None)))).await, StatusCode::BAD_REQUEST);
        assert_eq!(status(&app, call("POST", &roles, &boss, Some(grant(TEAM_ADMIN, Some("nope"))))).await, StatusCode::NOT_FOUND);
        assert_eq!(status(&app, call("POST", &roles, &boss, Some(grant(AUDITOR, Some("nope"))))).await, StatusCode::BAD_REQUEST);
        assert_eq!(status(&app, call("POST", &roles, &boss, Some(grant("owner", None)))).await, StatusCode::BAD_REQUEST);
        sqlx::query("INSERT INTO groups (id, external_id) VALUES ('g1', 'directory-g1')").execute(state.storage.pool()).await.unwrap();
        assert_eq!(status(&app, call("POST", &roles, &boss, Some(grant(TEAM_ADMIN, Some("g1"))))).await, StatusCode::OK);
        let me = json(&app, call("GET", "/api/me", &worker, None)).await;
        assert_eq!((me["roles"].clone(), me["team_admin_of"].clone()), (serde_json::json!(["team_admin"]), serde_json::json!(["g1"])));
        assert_eq!(status(&app, call("PATCH", "/api/admin/groups/g1", &boss, Some(serde_json::json!({ "name": "Engineering" })))).await, StatusCode::OK);
        let groups = json(&app, call("GET", "/api/admin/groups", &boss, None)).await;
        assert_eq!(groups[0]["name"], "Engineering");

        // Nobody withdraws their own admin role; roles from elsewhere are changed there.
        let own = format!("/api/admin/users/{admin}/roles");
        assert_eq!(status(&app, call("DELETE", &own, &boss, Some(grant(PLATFORM_ADMIN, None)))).await, StatusCode::CONFLICT);
        sqlx::query("INSERT INTO role_grants (user_id, role, source) VALUES ($1, 'auditor', 'directory')").bind(&user).execute(state.storage.pool()).await.unwrap();
        assert_eq!(status(&app, call("DELETE", &roles, &boss, Some(grant(AUDITOR, None)))).await, StatusCode::CONFLICT);
        assert_eq!(status(&app, call("DELETE", &roles, &boss, Some(grant(PLATFORM_ADMIN, None)))).await, StatusCode::NOT_FOUND);
    }

    #[test]
    fn what_the_directory_says_is_read_from_the_claims() {
        let read = Directory::from_claims(&serde_json::json!({ "groups": ["a", "b"], "roles": ["platform_admin", "auditor", "team_admin", "x"] }));
        assert_eq!(read.groups, Some(vec!["a".to_string(), "b".to_string()]));
        assert_eq!(read.roles, ["platform_admin", "auditor"], "team admin is per group, so never from the directory");
        assert_eq!(Directory::from_claims(&serde_json::json!({})).groups, Some(vec![]), "no groups claim: in no group");
        let over = Directory::from_claims(&serde_json::json!({ "_claim_names": { "groups": "src1" } }));
        assert_eq!(over.groups, None, "too many groups: the token says so instead of listing them");
        assert_eq!(Directory::from_claims(&serde_json::json!({ "hasgroups": true })).groups, None);
    }

    #[tokio::test]
    async fn a_sign_in_renews_directory_roles_and_keeps_grants_made_here() {
        let (_, state) = server();
        let pool = state.storage.pool();
        let (id, _) = person(&state, "eve", &[AUDITOR]).await;
        async fn renew(pool: &sqlx::PgPool, id: &str, groups: Option<Vec<&str>>, roles: Vec<&str>, admins: Vec<&str>) {
            let directory = Directory { groups: groups.map(|g| g.into_iter().map(String::from).collect()), roles: roles.into_iter().map(String::from).collect() };
            let admins: Vec<String> = admins.into_iter().map(String::from).collect();
            let mut connection = pool.acquire().await.unwrap();
            renew_at_sign_in(&mut connection, id, "Eve@Example.com", &directory, &admins).await.unwrap();
        }
        renew(pool, &id, Some(vec!["g1", "g2"]), vec![PLATFORM_ADMIN], vec![]).await;
        assert_eq!(grants(pool, &id).await.unwrap().0, ["auditor", "platform_admin"]);
        assert_eq!(groups_of(pool, &id).await.unwrap().len(), 2);
        renew(pool, &id, None, vec![], vec!["eve@example.com"]).await;
        assert_eq!(grants(pool, &id).await.unwrap().0, ["auditor", "platform_admin"], "the admin list, matched without case");
        assert_eq!(groups_of(pool, &id).await.unwrap().len(), 2, "groups left out of the token: memberships kept");
        renew(pool, &id, Some(vec!["g2"]), vec![], vec![]).await;
        assert_eq!(grants(pool, &id).await.unwrap().0, ["auditor"], "the grant made here stays; the others ended");
        assert_eq!(groups_of(pool, &id).await.unwrap().len(), 1);
    }

    #[test]
    fn the_admin_list_needs_sign_in() {
        // Through the configuration: without sign-in the local person is already the admin.
        let refused = crate::auth::Auth::from_values(|name| (name == "COMPANION_PLATFORM_ADMINS").then(|| "a@example.com".into()));
        assert!(refused.unwrap_err().contains("needs sign-in"));
        let auth = crate::auth::Auth::from_values(|name| {
            Some(match name {
                "COMPANION_PLATFORM_ADMINS" => " A@example.com; b@example.com ,".into(),
                "COMPANION_PUBLIC_URL" => "https://companion.example.com".into(),
                _ => "https://login.example.com/v2.0".into(),
            })
        })
        .unwrap();
        assert_eq!(auth.platform_admins, ["a@example.com", "b@example.com"]);
    }
}
