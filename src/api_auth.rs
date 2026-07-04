//! Bearer-token authentication for the JSON REST API.
//!
//! The HTML/htmx UI authenticates with a cookie-backed session
//! ([`axum_login::AuthSession`]), which is appropriate for a browser client
//! but not for a REST API: cookies are session-oriented, carry CSRF
//! concerns, and generally aren't how non-browser API clients authenticate.
//!
//! Instead, every `/api/v1/*` and `/admin/api/v1/*` endpoint authenticates
//! via a bearer token sent in the `Authorization: Bearer <token>` header.
//! Tokens are opaque random strings, issued by `POST /api/v1/login` and
//! persisted in the `api_tokens` table; [`ApiAuth`] is the extractor that
//! validates them.

use crate::errors::{ApiError, CustomError};
use crate::{AppState, CurrentUser};
use anyhow::anyhow;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::{header, request::Parts, StatusCode};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use scrypt::password_hash::rand_core::{OsRng, RngCore};
use sqlx::{query, Pool, Sqlite};
use time::{Duration, OffsetDateTime};

/// How long a freshly issued bearer token remains valid for.
const TOKEN_TTL: Duration = Duration::days(30);

/// Generate a new random, URL-safe opaque bearer token. 32 bytes of
/// `OsRng` output, base64-encoded - not tied to any user information, so
/// leaking one token can't be used to derive others.
fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Issue and persist a new bearer token for `user_id`. The raw token is
/// only ever available here, at issuance time - only its use (not its
/// value) can be observed afterwards.
pub async fn issue_token(pool: &Pool<Sqlite>, user_id: i64) -> Result<String, CustomError> {
    let token = generate_token();
    let now = OffsetDateTime::now_utc();
    let expires_at = now + TOKEN_TTL;

    query!(
        "insert into api_tokens (token, user_id, created_at, expires_at) values ($1, $2, $3, $4)",
        token,
        user_id,
        now,
        expires_at
    )
    .execute(pool)
    .await?;

    Ok(token)
}

/// Revoke a single bearer token (e.g. on logout). Revoking a token that
/// doesn't exist (or already expired) is not an error.
pub async fn revoke_token(pool: &Pool<Sqlite>, token: &str) -> Result<(), CustomError> {
    query!("delete from api_tokens where token = $1", token)
        .execute(pool)
        .await?;
    Ok(())
}

/// An axum extractor that authenticates a request via its
/// `Authorization: Bearer <token>` header, for use by JSON API handlers in
/// place of the cookie-based `AuthSession<Backend>` that the HTML/htmx UI
/// uses.
///
/// A token is only considered valid if it hasn't expired *and* the user it
/// belongs to is neither deactivated nor force-logged-out (the same checks
/// the cookie session backend applies) - so an admin forcibly logging a
/// worker out, or deactivating them, immediately invalidates their API
/// tokens too.
pub struct ApiAuth {
    pub user: CurrentUser,
    pub token: String,
}

impl<S> FromRequestParts<S> for ApiAuth
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AppState { pool, .. } = AppState::from_ref(state);

        let unauthorized = || {
            ApiError(CustomError::new(
                anyhow!("Missing or invalid Authorization header"),
                StatusCode::UNAUTHORIZED,
            ))
        };

        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(unauthorized)?
            .to_string();

        let now = OffsetDateTime::now_utc();

        let row = query!(
            r#"
            select users.id, users.name, users.admin
            from api_tokens
            inner join users on users.id = api_tokens.user_id
            where api_tokens.token = $1
                and api_tokens.expires_at > $2
                and users.deactivated = false
                and users.logged_out = false
            "#,
            token,
            now
        )
        .fetch_optional(&pool)
        .await
        .map_err(CustomError::from)?
        .ok_or_else(unauthorized)?;

        Ok(ApiAuth {
            user: CurrentUser {
                id: row.id,
                name: row.name,
                admin: row.admin,
            },
            token,
        })
    }
}
