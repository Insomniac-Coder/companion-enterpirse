//! Who is calling (Phase 1, task 3).
//!
//! Without sign-in configured, Companion serves only its own PC (it refuses any other address) and
//! every request is the one local person: a laptop install. With sign-in configured, people sign in
//! through the company's identity provider (OpenID Connect: Microsoft Entra ID or any standard one)
//! and their browser gets a session cookie; software sends one of a person's API keys instead.
//! Every /api/ request then carries a [`Caller`], except the few in [`OPEN`].

use crate::api::{ApiError, AppState};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Extension, Json};
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, Scope, TokenResponse,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::Row;

/// How long a sign-in lasts.
// ponytail: fixed; a company setting with task 6.
const SESSION_HOURS: i32 = 12;
/// How long the identity provider has to send a person back.
const ATTEMPT_MINUTES: i32 = 10;
const COOKIE: &str = "companion_session";
/// Ties a sign-in to the browser that started it: without it, someone could send a person the
/// return link of a sign-in they started themselves and so sign them in to their own account.
const STARTED: &str = "companion_sign_in";
/// The only /api/ routes that answer without knowing who is calling.
const OPEN: [&str; 3] = ["/api/health", "/api/auth/login", "/api/auth/callback"];

/// Sign-in through an OpenID Connect provider.
#[derive(Clone, Debug)]
pub struct OpenId {
    /// The provider's issuer, e.g. `https://login.microsoftonline.com/<tenant id>/v2.0`.
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    /// Where people reach this server, e.g. `https://companion.example.com`.
    pub public_url: reqwest::Url,
}

/// How this install tells who is calling.
#[derive(Clone, Debug, Default)]
pub struct Auth {
    /// None: no sign-in, one local person.
    pub open_id: Option<OpenId>,
}

const VARIABLES: [&str; 4] = [
    "COMPANION_OIDC_ISSUER",
    "COMPANION_OIDC_CLIENT_ID",
    "COMPANION_OIDC_CLIENT_SECRET",
    "COMPANION_PUBLIC_URL",
];

impl Auth {
    /// Sign-in from the environment: all four `VARIABLES`, or none of them (no sign-in).
    pub fn from_env() -> Result<Auth, String> {
        Self::from_values(|name| std::env::var(name).ok())
    }

    fn from_values(get: impl Fn(&str) -> Option<String>) -> Result<Auth, String> {
        let values: Vec<Option<String>> = VARIABLES.iter().map(|name| get(name).filter(|value| !value.trim().is_empty())).collect();
        if values.iter().all(Option::is_none) {
            return Ok(Auth::default());
        }
        let missing: Vec<&str> = VARIABLES.iter().zip(&values).filter(|(_, value)| value.is_none()).map(|(name, _)| *name).collect();
        if !missing.is_empty() {
            return Err(format!("sign-in is half configured: {} not set (set all four, or none)", missing.join(", ")));
        }
        let value = |index: usize| values[index].clone().unwrap_or_default().trim().to_string();
        let public_url = reqwest::Url::parse(value(3).trim_end_matches('/'))
            .map_err(|e| format!("COMPANION_PUBLIC_URL is not a URL: {e}"))?;
        if !matches!(public_url.scheme(), "https" | "http") || public_url.host_str().is_none() {
            return Err("COMPANION_PUBLIC_URL must be an http(s) address, e.g. https://companion.example.com".into());
        }
        IssuerUrl::new(value(0)).map_err(|e| format!("COMPANION_OIDC_ISSUER is not a URL: {e}"))?;
        Ok(Auth {
            open_id: Some(OpenId { issuer: value(0), client_id: value(1), client_secret: value(2), public_url }),
        })
    }

    pub fn sign_in_required(&self) -> bool {
        self.open_id.is_some()
    }

    /// The origin people's browsers use for this server, when it serves more than this PC.
    pub fn public_origin(&self) -> Option<String> {
        self.open_id.as_ref().map(|open_id| open_id.public_url.origin().ascii_serialization())
    }
}

/// An address other machines can reach needs sign-in: without it, anyone on the network would
/// be the local person.
pub fn check_address(address: &str, auth: &Auth) -> Result<(), String> {
    let host = address.rsplit_once(':').map(|(host, _)| host).unwrap_or(address);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let loopback = host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback());
    if loopback || auth.sign_in_required() {
        Ok(())
    } else {
        Err(format!(
            "{address} can be reached from other machines, which needs sign-in: set {} (see README), or listen on 127.0.0.1",
            VARIABLES.join(", ")
        ))
    }
}

/// The person (or their software) behind a request.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Caller {
    pub id: String,
    pub name: String,
    pub email: String,
    /// "local", "session" or "api key".
    pub via: &'static str,
}

impl Caller {
    /// The one person of an install without sign-in (the `users` row 'local').
    pub fn local() -> Caller {
        Caller { id: "local".into(), name: "You".into(), email: String::new(), via: "local" }
    }
}

fn hash(secret: &str) -> String {
    format!("{:x}", Sha256::digest(secret.as_bytes()))
}

/// 244 random bits as hex.
fn new_secret() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| pair.trim().strip_prefix(name).and_then(|rest| rest.strip_prefix('=')).map(str::to_string))
}

/// Who is calling, or None when the request proves nobody.
async fn caller(state: &AppState, headers: &HeaderMap) -> Result<Option<Caller>, sqlx::Error> {
    if !state.auth.sign_in_required() {
        return Ok(Some(Caller::local()));
    }
    let pool = state.storage.pool();
    // A request that names a key stands or falls by it; a cookie does not rescue a wrong key.
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        let Some(key) = value.to_str().ok().and_then(|value| value.strip_prefix("Bearer ")) else {
            return Ok(None);
        };
        let key_hash = hash(key.trim());
        let row = sqlx::query(
            "SELECT u.id, u.name, u.email FROM api_keys k JOIN users u ON u.id = k.user_id
             WHERE k.key_hash = $1 AND k.revoked_at IS NULL",
        )
        .bind(&key_hash)
        .fetch_optional(pool)
        .await?;
        if row.is_some() {
            sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE key_hash = $1 AND (last_used_at IS NULL OR last_used_at < now() - interval '1 minute')")
                .bind(&key_hash)
                .execute(pool)
                .await?;
        }
        return Ok(row.map(|row| Caller { id: row.get("id"), name: row.get("name"), email: row.get("email"), via: "api key" }));
    }
    let Some(token) = cookie(headers, COOKIE) else {
        return Ok(None);
    };
    let row = sqlx::query(
        "SELECT u.id, u.name, u.email FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = $1 AND s.expires_at > now()",
    )
    .bind(hash(&token))
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| Caller { id: row.get("id"), name: row.get("name"), email: row.get("email"), via: "session" }))
}

/// Every /api/ request but the `OPEN` ones must come from someone.
pub async fn authenticate(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    let path = request.uri().path();
    if !path.starts_with("/api/") || OPEN.contains(&path) {
        return next.run(request).await;
    }
    match caller(&state, request.headers()).await {
        Ok(Some(caller)) => {
            request.extensions_mut().insert(caller);
            next.run(request).await
        }
        Ok(None) => ApiError::new(StatusCode::UNAUTHORIZED, "sign in required", "Sign in with your company account.").into_response(),
        Err(error) => ApiError::internal(format!("cannot check who is calling: {error}")).into_response(),
    }
}

/// Only a path on this server: never another site (`//host`, `/\host`, a full URL).
fn safe_return(path: Option<&str>) -> String {
    match path {
        Some(path) if path.starts_with('/') && !path.starts_with("//") && !path.contains('\\') && !path.chars().any(char::is_control) => path.to_string(),
        _ => "/".to_string(),
    }
}

fn http_client() -> Result<openidconnect::reqwest::Client, String> {
    openidconnect::reqwest::ClientBuilder::new()
        // Following redirects would let a provider send these requests anywhere.
        .redirect(openidconnect::reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("cannot make an HTTP client: {e}"))
}

/// The provider's current endpoints and signing keys. Fetched for each sign-in, so a key the
/// provider rotated in is always known (sign-ins are a few a day per person).
async fn discover(open_id: &OpenId, http: &openidconnect::reqwest::Client) -> Result<CoreProviderMetadata, String> {
    let issuer = IssuerUrl::new(open_id.issuer.clone()).map_err(|e| e.to_string())?;
    CoreProviderMetadata::discover_async(issuer, http)
        .await
        .map_err(|e| format!("cannot reach the identity provider at {}: {e}", open_id.issuer))
}

fn redirect_url(open_id: &OpenId) -> Result<RedirectUrl, String> {
    RedirectUrl::new(format!("{}/api/auth/callback", open_id.public_url.as_str().trim_end_matches('/'))).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
pub struct LoginQuery {
    return_to: Option<String>,
}

/// Start a sign-in: off to the identity provider, which sends the person back to `callback`.
pub async fn login(State(state): State<AppState>, Query(query): Query<LoginQuery>) -> Response {
    let return_to = safe_return(query.return_to.as_deref());
    let Some(open_id) = &state.auth.open_id else {
        return Redirect::to(&return_to).into_response();
    };
    match start_sign_in(&state, open_id, &return_to).await {
        Ok((url, csrf)) => {
            let mut response = Redirect::to(&url).into_response();
            let started = format!("{STARTED}={csrf}; Path=/api/auth; HttpOnly; SameSite=Lax; Max-Age={}{}", ATTEMPT_MINUTES * 60, secure(open_id));
            if let Ok(value) = started.parse() {
                response.headers_mut().append(header::SET_COOKIE, value);
            }
            response
        }
        Err(reason) => sign_in_failed(&reason),
    }
}

fn secure(open_id: &OpenId) -> &'static str {
    if open_id.public_url.scheme() == "https" { "; Secure" } else { "" }
}

/// The URL to send the browser to, and the state that must come back with it.
async fn start_sign_in(state: &AppState, open_id: &OpenId, return_to: &str) -> Result<(String, String), String> {
    let http = http_client()?;
    let metadata = discover(open_id, &http).await?;
    let client = CoreClient::from_provider_metadata(metadata, ClientId::new(open_id.client_id.clone()), Some(ClientSecret::new(open_id.client_secret.clone())))
        .set_redirect_uri(redirect_url(open_id)?);
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, csrf, nonce) = client
        .authorize_url(CoreAuthenticationFlow::AuthorizationCode, CsrfToken::new_random, Nonce::new_random)
        .add_scope(Scope::new("email".into()))
        .add_scope(Scope::new("profile".into()))
        .set_pkce_challenge(challenge)
        .url();
    let pool = state.storage.pool();
    let stored = async {
        sqlx::query("DELETE FROM sign_in_attempts WHERE created_at < now() - make_interval(mins => $1)")
            .bind(ATTEMPT_MINUTES)
            .execute(pool)
            .await?;
        sqlx::query("INSERT INTO sign_in_attempts (state, nonce, pkce_verifier, return_to) VALUES ($1, $2, $3, $4)")
            .bind(csrf.secret())
            .bind(nonce.secret())
            .bind(verifier.secret())
            .bind(return_to)
            .execute(pool)
            .await
    };
    stored.await.map_err(|e| format!("cannot record the sign-in: {e}"))?;
    Ok((url.to_string(), csrf.secret().clone()))
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// The identity provider sends the person back here: check everything it says, then sign in.
pub async fn callback(State(state): State<AppState>, headers: HeaderMap, Query(query): Query<CallbackQuery>) -> Response {
    let Some(open_id) = state.auth.open_id.clone() else {
        return Redirect::to("/").into_response();
    };
    let mut response = match finish_sign_in(&state, &open_id, &headers, query).await {
        Ok((token, return_to)) => {
            let mut response = Redirect::to(&return_to).into_response();
            let session = format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}", SESSION_HOURS * 3600, secure(&open_id));
            if let Ok(value) = session.parse() {
                response.headers_mut().append(header::SET_COOKIE, value);
            }
            response
        }
        Err(reason) => {
            tracing::warn!(%reason, "sign-in refused");
            sign_in_failed(&reason)
        }
    };
    // The sign-in is over either way.
    if let Ok(value) = format!("{STARTED}=; Path=/api/auth; HttpOnly; SameSite=Lax; Max-Age=0").parse() {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

async fn finish_sign_in(state: &AppState, open_id: &OpenId, headers: &HeaderMap, query: CallbackQuery) -> Result<(String, String), String> {
    if let Some(error) = query.error {
        // Only the provider's short code is shown: the link's own words could say anything.
        tracing::warn!(%error, description = query.error_description.unwrap_or_default(), "the identity provider refused a sign-in");
        let code: String = error.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(40).collect();
        return Err(format!("the identity provider refused it ({code})"));
    }
    let (Some(code), Some(csrf)) = (query.code, query.state) else {
        return Err("the identity provider sent no sign-in code".into());
    };
    if cookie(headers, STARTED).as_deref() != Some(csrf.as_str()) {
        return Err("this sign-in was not started in this browser: start it again here".into());
    }
    let pool = state.storage.pool();
    // Used once: a second visit to the same link finds nothing.
    let attempt = sqlx::query(
        "DELETE FROM sign_in_attempts WHERE state = $1 AND created_at > now() - make_interval(mins => $2)
         RETURNING nonce, pkce_verifier, return_to",
    )
    .bind(&csrf)
    .bind(ATTEMPT_MINUTES)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("cannot read the sign-in: {e}"))?
    .ok_or("this sign-in link was already used or has expired")?;
    let (nonce, verifier, return_to): (String, String, String) = (attempt.get("nonce"), attempt.get("pkce_verifier"), attempt.get("return_to"));

    let http = http_client()?;
    let metadata = discover(open_id, &http).await?;
    let client = CoreClient::from_provider_metadata(metadata, ClientId::new(open_id.client_id.clone()), Some(ClientSecret::new(open_id.client_secret.clone())))
        .set_redirect_uri(redirect_url(open_id)?);
    let tokens = client
        .exchange_code(AuthorizationCode::new(code))
        .map_err(|e| format!("the identity provider has no token endpoint: {e}"))?
        .set_pkce_verifier(PkceCodeVerifier::new(verifier))
        .request_async(&http)
        .await
        .map_err(|e| format!("the identity provider did not confirm the sign-in: {e}"))?;
    let id_token = tokens.id_token().ok_or("the identity provider sent no ID token")?;
    // Signature (the provider's published keys), issuer, audience, expiry and nonce.
    let claims = id_token
        .claims(&client.id_token_verifier(), &Nonce::new(nonce))
        .map_err(|e| format!("the ID token was refused: {e}"))?;
    let email = claims
        .email()
        .map(|email| email.as_str().to_string())
        .or_else(|| claims.preferred_username().map(|name| name.as_str().to_string()))
        .unwrap_or_default();
    let name = claims.name().and_then(|name| name.get(None)).map(|name| name.as_str().to_string()).unwrap_or_else(|| email.clone());

    let user_id: String = sqlx::query_scalar(
        "INSERT INTO users (id, issuer, subject, email, name) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (issuer, subject) DO UPDATE SET email = EXCLUDED.email, name = EXCLUDED.name, last_seen_at = now()
         RETURNING id",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(claims.issuer().as_str())
    .bind(claims.subject().as_str())
    .bind(&email)
    .bind(&name)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("cannot save the person: {e}"))?;
    let token = new_secret();
    sqlx::query("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, $2, now() + make_interval(hours => $3))")
        .bind(hash(&token))
        .bind(&user_id)
        .bind(SESSION_HOURS)
        .execute(pool)
        .await
        .map_err(|e| format!("cannot start the session: {e}"))?;
    tracing::info!(user = %user_id, "signed in");
    Ok((token, return_to))
}

fn sign_in_failed(reason: &str) -> Response {
    let reason = reason.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    (
        StatusCode::BAD_REQUEST,
        Html(format!(
            "<!doctype html><meta charset=utf-8><title>Sign-in did not finish</title>\
             <body style=\"font-family:system-ui,sans-serif;max-width:36rem;margin:4rem auto;padding:0 1rem\">\
             <h1 style=\"font-size:1.3rem\">Sign-in did not finish</h1><p>{reason}</p>\
             <p><a href=\"/api/auth/login\">Try again</a></p></body>"
        )),
    )
        .into_response()
}

/// End this browser's session.
pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    if let Some(token) = cookie(&headers, COOKIE) {
        sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(hash(&token))
            .execute(state.storage.pool())
            .await
            .map_err(|e| ApiError::internal(format!("cannot end the session: {e}")))?;
    }
    let clear = format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    Ok(([(header::SET_COOKIE, clear)], Json(serde_json::json!({ "signed_out": true }))).into_response())
}

/// Who this is, and whether this install has sign-in.
pub async fn me(State(state): State<AppState>, Extension(caller): Extension<Caller>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "id": caller.id,
        "name": caller.name,
        "email": caller.email,
        "via": caller.via,
        "sign_in": state.auth.sign_in_required(),
    }))
}

/// API keys are made and withdrawn from a signed-in browser only: a key cannot make more keys.
fn key_owner(state: &AppState, caller: &Caller) -> Result<(), ApiError> {
    if !state.auth.sign_in_required() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "this install has no sign-in",
            "Software on this PC can call Companion without a key.",
        ));
    }
    if caller.via != "session" {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "API keys are managed from a signed-in browser", "Sign in to Companion and manage keys in Settings."));
    }
    Ok(())
}

pub async fn list_api_keys(State(state): State<AppState>, Extension(caller): Extension<Caller>) -> Result<Json<serde_json::Value>, ApiError> {
    key_owner(&state, &caller)?;
    let rows = sqlx::query(
        "SELECT id, name, hint, created_at::TEXT AS created_at, last_used_at::TEXT AS last_used_at, revoked_at::TEXT AS revoked_at
         FROM api_keys WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(&caller.id)
    .fetch_all(state.storage.pool())
    .await
    .map_err(|e| ApiError::internal(format!("cannot list API keys: {e}")))?;
    Ok(Json(serde_json::Value::Array(
        rows.iter()
            .map(|row| {
                serde_json::json!({
                    "id": row.get::<String, _>("id"),
                    "name": row.get::<String, _>("name"),
                    "hint": row.get::<String, _>("hint"),
                    "created_at": row.get::<String, _>("created_at"),
                    "last_used_at": row.get::<Option<String>, _>("last_used_at"),
                    "revoked_at": row.get::<Option<String>, _>("revoked_at"),
                })
            })
            .collect(),
    )))
}

#[derive(Deserialize)]
pub struct NewApiKey {
    name: String,
}

/// A new key, shown this once.
pub async fn create_api_key(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<NewApiKey>,
) -> Result<Json<serde_json::Value>, ApiError> {
    key_owner(&state, &caller)?;
    let name = request.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(ApiError::bad("the key needs a name of 1 to 80 characters", "Name it after the software that will use it."));
    }
    let key = format!("cmp_{}", new_secret());
    let hint = format!("{}…", &key[..12]);
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO api_keys (id, user_id, name, key_hash, hint) VALUES ($1, $2, $3, $4, $5)")
        .bind(&id)
        .bind(&caller.id)
        .bind(name)
        .bind(hash(&key))
        .bind(&hint)
        .execute(state.storage.pool())
        .await
        .map_err(|e| ApiError::internal(format!("cannot make the key: {e}")))?;
    Ok(Json(serde_json::json!({ "id": id, "name": name, "hint": hint, "key": key })))
}

/// Withdraw one of the caller's keys; it stops working at once.
pub async fn revoke_api_key(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    key_owner(&state, &caller)?;
    let done = sqlx::query("UPDATE api_keys SET revoked_at = now() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL")
        .bind(&id)
        .bind(&caller.id)
        .execute(state.storage.pool())
        .await
        .map_err(|e| ApiError::internal(format!("cannot withdraw the key: {e}")))?;
    if done.rows_affected() == 0 {
        return Err(ApiError::not_found("no such key of yours, or it is already withdrawn"));
    }
    Ok(Json(serde_json::json!({ "revoked": id })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use openidconnect::core::{CoreIdToken, CoreIdTokenClaims, CoreJsonWebKeySet, CoreJwsSigningAlgorithm, CoreRsaPrivateSigningKey};
    use openidconnect::{
        Audience, EmptyAdditionalClaims, EndUserEmail, EndUserName, JsonWebKeyId, LocalizedClaim, PrivateSigningKey, StandardClaims,
        SubjectIdentifier,
    };
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    use tower::ServiceExt as _;

    // --- A stand-in identity provider: discovery, signing keys, and a token endpoint that checks PKCE. ---

    /// How the stand-in spoils the ID token it hands out.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Spoil {
        Nothing,
        Nonce,
        SignedByAnotherKey,
        Audience,
        Expired,
        Issuer,
    }

    struct Grant {
        nonce: String,
        challenge: String,
        subject: String,
        spoil: Spoil,
    }

    struct Provider {
        issuer: String,
        grants: Mutex<HashMap<String, Grant>>,
    }

    /// Two RSA keys, made once: the provider's, and a stranger's.
    fn keys() -> &'static [String; 2] {
        static KEYS: OnceLock<[String; 2]> = OnceLock::new();
        KEYS.get_or_init(|| {
            use rsa::pkcs1::EncodeRsaPrivateKey;
            let pem = || {
                rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048)
                    .unwrap()
                    .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
                    .unwrap()
                    .to_string()
            };
            [pem(), pem()]
        })
    }

    fn signing_key(index: usize) -> CoreRsaPrivateSigningKey {
        CoreRsaPrivateSigningKey::from_pem(&keys()[index], Some(JsonWebKeyId::new("key-1".into()))).unwrap()
    }

    const CLIENT_ID: &str = "companion";
    const CLIENT_SECRET: &str = "client-secret";

    async fn start_provider() -> Arc<Provider> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let provider = Arc::new(Provider { issuer: issuer.clone(), grants: Mutex::new(HashMap::new()) });
        let discovery = serde_json::json!({
            "issuer": issuer,
            "authorization_endpoint": format!("{issuer}/authorize"),
            "token_endpoint": format!("{issuer}/token"),
            "jwks_uri": format!("{issuer}/jwks"),
            "response_types_supported": ["code"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["RS256"],
        });
        let shared = provider.clone();
        let app = axum::Router::new()
            .route("/.well-known/openid-configuration", axum::routing::get(move || async move { Json(discovery) }))
            .route(
                "/jwks",
                axum::routing::get(|| async { Json(CoreJsonWebKeySet::new(vec![signing_key(0).as_verification_key()])) }),
            )
            .route(
                "/token",
                axum::routing::post(move |headers: HeaderMap, axum::Form(form): axum::Form<HashMap<String, String>>| {
                    let provider = shared.clone();
                    async move { provider.token(&headers, &form) }
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        provider
    }

    impl Provider {
        fn token(&self, headers: &HeaderMap, form: &HashMap<String, String>) -> Response {
            use base64::Engine as _;
            let refuse = |why: &str| {
                (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "invalid_grant", "error_description": why }))).into_response()
            };
            let client = base64::engine::general_purpose::STANDARD.encode(format!("{CLIENT_ID}:{CLIENT_SECRET}"));
            if headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()) != Some(format!("Basic {client}").as_str()) {
                return refuse("wrong client credentials");
            }
            let Some(grant) = form.get("code").and_then(|code| self.grants.lock().unwrap().remove(code)) else {
                return refuse("unknown code");
            };
            let verifier = form.get("code_verifier").cloned().unwrap_or_default();
            let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
            if challenge != grant.challenge {
                return refuse("PKCE verifier does not match");
            }
            let now = chrono::Utc::now();
            let (issued, expires) = if grant.spoil == Spoil::Expired {
                (now - chrono::Duration::hours(3), now - chrono::Duration::hours(2))
            } else {
                (now, now + chrono::Duration::hours(1))
            };
            let issuer = if grant.spoil == Spoil::Issuer { "http://impostor.test".to_string() } else { self.issuer.clone() };
            let audience = if grant.spoil == Spoil::Audience { "another-app" } else { CLIENT_ID };
            let nonce = if grant.spoil == Spoil::Nonce { "a-different-nonce".to_string() } else { grant.nonce };
            let claims = CoreIdTokenClaims::new(
                IssuerUrl::new(issuer).unwrap(),
                vec![Audience::new(audience.into())],
                expires,
                issued,
                StandardClaims::new(SubjectIdentifier::new(grant.subject.clone()))
                    .set_email(Some(EndUserEmail::new(format!("{}@example.com", grant.subject))))
                    .set_name(Some(LocalizedClaim::from(EndUserName::new(format!("Person {}", grant.subject))))),
                EmptyAdditionalClaims {},
            )
            .set_nonce(Some(Nonce::new(nonce)));
            let key = signing_key(if grant.spoil == Spoil::SignedByAnotherKey { 1 } else { 0 });
            let id_token = CoreIdToken::new(claims, &key, CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256, None, None).unwrap();
            Json(serde_json::json!({ "access_token": "access", "token_type": "Bearer", "expires_in": 3600, "id_token": id_token }))
                .into_response()
        }
    }

    // --- Companion with sign-in pointed at the stand-in. ---

    const PUBLIC_URL: &str = "https://companion.example.com";

    async fn companion_with_sign_in() -> (axum::Router, Arc<Provider>, AppState) {
        let provider = start_provider().await;
        let auth = Auth {
            open_id: Some(OpenId {
                issuer: provider.issuer.clone(),
                client_id: CLIENT_ID.into(),
                client_secret: CLIENT_SECRET.into(),
                public_url: reqwest::Url::parse(PUBLIC_URL).unwrap(),
            }),
        };
        let state = AppState::new_stub().with_auth(auth);
        (crate::api::router(state.clone()), provider, state)
    }

    fn get(uri: &str) -> Request<Body> {
        Request::builder().uri(uri).body(Body::empty()).unwrap()
    }

    fn with_header(mut request: Request<Body>, name: &str, value: &str) -> Request<Body> {
        request.headers_mut().insert(axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(), value.parse().unwrap());
        request
    }

    async fn send(app: &axum::Router, request: Request<Body>) -> Response {
        app.clone().oneshot(request).await.unwrap()
    }

    async fn body_json(response: Response) -> serde_json::Value {
        serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap()
    }

    fn location(response: &Response) -> String {
        response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string()
    }

    /// The cookie named `name` that a response sets, with its attributes.
    fn cookie_set(response: &Response, name: &str) -> Option<String> {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap().to_string())
            .find(|cookie| cookie.starts_with(&format!("{name}=")))
    }

    fn set_cookie(response: &Response) -> Option<String> {
        cookie_set(response, COOKIE)
    }

    fn session_of(set_cookie: &str) -> String {
        set_cookie.split(';').next().unwrap().to_string()
    }

    /// Start a sign-in, let the stand-in approve `subject` (spoiling the token as asked), and
    /// return Companion's answer when the browser comes back, the link it came back on, and the
    /// cookie that ties the sign-in to that browser.
    async fn sign_in(app: &axum::Router, provider: &Provider, subject: &str, spoil: Spoil) -> (Response, String, String) {
        let started = send(app, get("/api/auth/login?return_to=/projects")).await;
        assert_eq!(started.status(), StatusCode::SEE_OTHER);
        let browser = cookie_set(&started, STARTED).expect("the sign-in is tied to this browser");
        for attribute in ["HttpOnly", "SameSite=Lax", "Secure", "Path=/api/auth", "Max-Age=600"] {
            assert!(browser.contains(attribute), "{attribute} in {browser}");
        }
        let browser = session_of(&browser);
        let url = reqwest::Url::parse(&location(&started)).unwrap();
        let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(url.path(), "/authorize");
        assert_eq!(query["client_id"], CLIENT_ID);
        assert_eq!(query["redirect_uri"], format!("{PUBLIC_URL}/api/auth/callback"));
        assert_eq!(query["code_challenge_method"], "S256", "PKCE");
        assert!(query["scope"].contains("openid"));
        let code = format!("code-{}", uuid::Uuid::new_v4().simple());
        provider.grants.lock().unwrap().insert(
            code.clone(),
            Grant { nonce: query["nonce"].clone(), challenge: query["code_challenge"].clone(), subject: subject.into(), spoil },
        );
        let back = format!("/api/auth/callback?code={code}&state={}", query["state"]);
        (send(app, with_header(get(&back), "cookie", &browser)).await, back, browser)
    }

    // --- Configuration and addresses. ---

    #[test]
    fn sign_in_is_all_four_settings_or_none() {
        assert!(!Auth::from_values(|_| None).unwrap().sign_in_required());
        let half = Auth::from_values(|name| (name == "COMPANION_OIDC_ISSUER").then(|| "https://login.example.com/v2.0".to_string())).unwrap_err();
        assert!(half.contains("COMPANION_OIDC_CLIENT_ID") && half.contains("COMPANION_PUBLIC_URL"), "{half}");
        let all = Auth::from_values(|name| {
            Some(match name {
                "COMPANION_OIDC_ISSUER" => "https://login.example.com/v2.0".to_string(),
                "COMPANION_PUBLIC_URL" => "https://companion.example.com/".to_string(),
                _ => "x".to_string(),
            })
        })
        .unwrap();
        assert!(all.sign_in_required());
        assert_eq!(all.public_origin().as_deref(), Some("https://companion.example.com"));
        let not_a_url = Auth::from_values(|name| Some(if name == "COMPANION_PUBLIC_URL" { "companion".into() } else { "https://login.example.com".into() }));
        assert!(not_a_url.is_err());
    }

    #[test]
    fn only_this_pc_may_be_served_without_sign_in() {
        let none = Auth::default();
        for local in ["127.0.0.1:5173", "localhost:3877", "[::1]:3877", "127.0.0.2:80"] {
            assert!(check_address(local, &none).is_ok(), "{local}");
        }
        for open in ["0.0.0.0:3877", "192.168.1.20:3877", "[::]:3877", "companion.example.com:443"] {
            let refused = check_address(open, &none).unwrap_err();
            assert!(refused.contains("needs sign-in"), "{open}: {refused}");
        }
        let signed = Auth::from_values(|name| Some(if name == "COMPANION_PUBLIC_URL" { PUBLIC_URL.into() } else { "https://login.example.com".into() }))
            .unwrap();
        assert!(check_address("0.0.0.0:3877", &signed).is_ok());
    }

    #[test]
    fn a_return_address_never_leaves_this_server() {
        assert_eq!(safe_return(Some("/projects?id=4")), "/projects?id=4");
        for away in ["//evil.example", "https://evil.example", "/\\evil.example", "evil", "/a\nb", ""] {
            assert_eq!(safe_return(Some(away)), "/", "{away:?}");
        }
        assert_eq!(safe_return(None), "/");
    }

    // --- Without sign-in. ---

    #[tokio::test]
    async fn without_sign_in_every_request_is_the_local_person() {
        let app = crate::api::router(AppState::new_stub());
        let me = body_json(send(&app, get("/api/me")).await).await;
        assert_eq!((me["id"].as_str(), me["via"].as_str(), me["sign_in"].as_bool()), (Some("local"), Some("local"), Some(false)));
        assert_eq!(send(&app, get("/api/conversations")).await.status(), StatusCode::OK);
        assert_eq!(send(&app, get("/api/me/api-keys")).await.status(), StatusCode::CONFLICT, "no keys without sign-in");
        assert_eq!(location(&send(&app, get("/api/auth/login?return_to=/x")).await), "/x", "nothing to sign in to");
    }

    // --- With sign-in. ---

    #[tokio::test]
    async fn a_sign_in_gives_a_session_that_ends_at_sign_out() {
        let (app, provider, _) = companion_with_sign_in().await;
        assert_eq!(send(&app, get("/api/conversations")).await.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(send(&app, get("/api/health")).await.status(), StatusCode::OK, "health answers anyone");

        let (back, link, browser) = sign_in(&app, &provider, "ada", Spoil::Nothing).await;
        assert_eq!(back.status(), StatusCode::SEE_OTHER);
        assert_eq!(location(&back), "/projects");
        let cookie = set_cookie(&back).expect("a session cookie");
        for attribute in ["HttpOnly", "SameSite=Lax", "Secure", "Path=/"] {
            assert!(cookie.contains(attribute), "{attribute} in {cookie}");
        }
        let session = session_of(&cookie);
        let me = body_json(send(&app, with_header(get("/api/me"), "cookie", &session)).await).await;
        assert_eq!((me["name"].as_str(), me["email"].as_str(), me["via"].as_str()), (Some("Person ada"), Some("ada@example.com"), Some("session")));
        assert_eq!(send(&app, with_header(get("/api/conversations"), "cookie", &session)).await.status(), StatusCode::OK);

        let again = send(&app, with_header(get(&link), "cookie", &browser)).await;
        assert_eq!(again.status(), StatusCode::BAD_REQUEST, "a sign-in link works once");
        assert!(set_cookie(&again).is_none());

        // Signing in again later is the same person, not a new one.
        let (second, _, _) = sign_in(&app, &provider, "ada", Spoil::Nothing).await;
        let other = session_of(&set_cookie(&second).unwrap());
        assert_eq!(body_json(send(&app, with_header(get("/api/me"), "cookie", &other)).await).await["id"], me["id"]);

        let out = Request::builder().method("POST").uri("/api/auth/logout").header("cookie", &session).body(Body::empty()).unwrap();
        assert!(set_cookie(&send(&app, out).await).unwrap().contains("Max-Age=0"));
        assert_eq!(send(&app, with_header(get("/api/me"), "cookie", &session)).await.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(send(&app, with_header(get("/api/me"), "cookie", &other)).await.status(), StatusCode::OK, "only that browser signed out");
    }

    #[tokio::test]
    async fn an_id_token_that_fails_any_check_signs_nobody_in() {
        let (app, provider, _) = companion_with_sign_in().await;
        for spoil in [Spoil::Nonce, Spoil::SignedByAnotherKey, Spoil::Audience, Spoil::Expired, Spoil::Issuer] {
            let (back, _, _) = sign_in(&app, &provider, "mallory", spoil).await;
            assert_eq!(back.status(), StatusCode::BAD_REQUEST, "{spoil:?}");
            assert!(set_cookie(&back).is_none(), "{spoil:?} must not sign in");
            let page = String::from_utf8(axum::body::to_bytes(back.into_body(), 100_000).await.unwrap().to_vec()).unwrap();
            assert!(page.contains("the ID token was refused"), "{spoil:?} refused by the token check, not before it: {page}");
        }
    }

    #[tokio::test]
    async fn a_refusal_or_a_stale_link_signs_nobody_in() {
        let (app, provider, state) = companion_with_sign_in().await;
        let refused = send(&app, get("/api/auth/callback?error=access_denied&error_description=Call%20555-0100%20to%20unlock")).await;
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
        assert!(set_cookie(&refused).is_none());
        let page = String::from_utf8(axum::body::to_bytes(refused.into_body(), 100_000).await.unwrap().to_vec()).unwrap();
        assert!(page.contains("(access_denied)") && !page.contains("555"), "only the code is shown, not the link's words: {page}");
        assert_eq!(send(&app, get("/api/auth/callback?code=x&state=made-up")).await.status(), StatusCode::BAD_REQUEST);

        // A link older than ten minutes.
        let started = send(&app, get("/api/auth/login")).await;
        let browser = session_of(&cookie_set(&started, STARTED).unwrap());
        let url = reqwest::Url::parse(&location(&started)).unwrap();
        let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
        sqlx::query("UPDATE sign_in_attempts SET created_at = now() - interval '11 minutes'").execute(state.storage.pool()).await.unwrap();
        provider.grants.lock().unwrap().insert(
            "late".into(),
            Grant { nonce: query["nonce"].clone(), challenge: query["code_challenge"].clone(), subject: "late".into(), spoil: Spoil::Nothing },
        );
        let late = send(&app, with_header(get(&format!("/api/auth/callback?code=late&state={}", query["state"])), "cookie", &browser)).await;
        assert_eq!(late.status(), StatusCode::BAD_REQUEST);
        assert!(set_cookie(&late).is_none());
    }

    #[tokio::test]
    async fn a_return_link_works_only_in_the_browser_that_started_the_sign_in() {
        let (app, provider, state) = companion_with_sign_in().await;
        // Mallory starts a sign-in as herself and gets as far as the return link...
        let started = send(&app, get("/api/auth/login")).await;
        let url = reqwest::Url::parse(&location(&started)).unwrap();
        let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
        provider.grants.lock().unwrap().insert(
            "mallorys".into(),
            Grant { nonce: query["nonce"].clone(), challenge: query["code_challenge"].clone(), subject: "mallory".into(), spoil: Spoil::Nothing },
        );
        let link = format!("/api/auth/callback?code=mallorys&state={}", query["state"]);
        // ...and sends it to Ada, whose browser never started a sign-in, or started its own.
        for ada in [get(&link), with_header(get(&link), "cookie", &format!("{STARTED}=her-own-sign-in"))] {
            let back = send(&app, ada).await;
            assert_eq!(back.status(), StatusCode::BAD_REQUEST);
            assert!(set_cookie(&back).is_none(), "Ada is not signed in as Mallory");
        }
        let signed_in: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions").fetch_one(state.storage.pool()).await.unwrap();
        assert_eq!(signed_in, 0);
    }

    #[tokio::test]
    async fn an_expired_session_is_refused() {
        let (app, provider, state) = companion_with_sign_in().await;
        let (back, _, _) = sign_in(&app, &provider, "grace", Spoil::Nothing).await;
        let session = session_of(&set_cookie(&back).unwrap());
        sqlx::query("UPDATE sessions SET expires_at = now() - interval '1 second'").execute(state.storage.pool()).await.unwrap();
        assert_eq!(send(&app, with_header(get("/api/me"), "cookie", &session)).await.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_keys_work_until_withdrawn_and_only_their_hash_is_kept() {
        let (app, provider, state) = companion_with_sign_in().await;
        let (back, _, _) = sign_in(&app, &provider, "linus", Spoil::Nothing).await;
        let session = session_of(&set_cookie(&back).unwrap());
        let create = |name: &str| {
            Request::builder()
                .method("POST")
                .uri("/api/me/api-keys")
                .header("cookie", &session)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({ "name": name }).to_string()))
                .unwrap()
        };
        assert_eq!(send(&app, create("  ")).await.status(), StatusCode::BAD_REQUEST, "a key needs a name");
        let made = body_json(send(&app, create("Build server")).await).await;
        let key = made["key"].as_str().unwrap().to_string();
        assert!(key.starts_with("cmp_") && made["hint"].as_str().unwrap().ends_with('…'));

        let bearer = |key: &str, uri: &str| with_header(get(uri), "authorization", &format!("Bearer {key}"));
        let me = body_json(send(&app, bearer(&key, "/api/me")).await).await;
        assert_eq!((me["name"].as_str(), me["via"].as_str()), (Some("Person linus"), Some("api key")));
        assert_eq!(send(&app, bearer(&key, "/api/me/api-keys")).await.status(), StatusCode::FORBIDDEN, "a key cannot manage keys");
        assert_eq!(send(&app, bearer("cmp_wrong", "/api/me")).await.status(), StatusCode::UNAUTHORIZED);
        let wrong_key_good_cookie = with_header(bearer("cmp_wrong", "/api/me"), "cookie", &session);
        assert_eq!(send(&app, wrong_key_good_cookie).await.status(), StatusCode::UNAUTHORIZED, "a wrong key is not rescued by a cookie");

        let stored: Vec<String> = sqlx::query_scalar("SELECT key_hash || ' ' || hint FROM api_keys").fetch_all(state.storage.pool()).await.unwrap();
        assert!(stored.iter().all(|row| !row.contains(&key[12..])), "the key itself is not stored");

        let listed = body_json(send(&app, with_header(get("/api/me/api-keys"), "cookie", &session)).await).await;
        assert_eq!(listed[0]["name"], "Build server");
        assert!(listed[0]["last_used_at"].is_string(), "use is recorded");
        assert!(listed[0].get("key").is_none(), "a key is shown only once");

        let withdraw = Request::builder()
            .method("DELETE")
            .uri(format!("/api/me/api-keys/{}", made["id"].as_str().unwrap()))
            .header("cookie", &session)
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(&app, withdraw).await.status(), StatusCode::OK);
        assert_eq!(send(&app, bearer(&key, "/api/me")).await.status(), StatusCode::UNAUTHORIZED, "withdrawn at once");

        // Someone else sees none of it.
        let (other, _, _) = sign_in(&app, &provider, "eve", Spoil::Nothing).await;
        let eve = session_of(&set_cookie(&other).unwrap());
        assert_eq!(body_json(send(&app, with_header(get("/api/me/api-keys"), "cookie", &eve)).await).await, serde_json::json!([]));
    }

    #[tokio::test]
    async fn browsers_on_other_sites_are_refused_and_the_public_address_is_served() {
        let (app, _, _) = companion_with_sign_in().await;
        let from = |origin: &str, host: &str| with_header(with_header(get("/api/health"), "origin", origin), "host", host);
        assert_eq!(send(&app, from(PUBLIC_URL, "companion.example.com")).await.status(), StatusCode::OK);
        assert_eq!(send(&app, from("https://evil.example", "companion.example.com")).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(send(&app, with_header(get("/api/health"), "host", "evil.example")).await.status(), StatusCode::FORBIDDEN, "DNS rebinding");
    }
}
