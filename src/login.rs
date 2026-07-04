use std::convert::Infallible;

use crate::api_auth::{self, ApiAuth};
use crate::errors::{to_api, ApiError, CustomError};
use crate::{current_user, get_admin};
use crate::AppState;

use super::Worker;

use crate::Backend;
use anyhow::anyhow;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::Form;
use axum::Json;
use axum_login::AuthSession;
use axum_login::AuthnBackend;
use axum_template::RenderHtml;
use git_version::git_version;
use scrypt::{
    password_hash::{PasswordHasher, SaltString},
    Scrypt,
};
use serde::{Deserialize, Serialize};
use sqlx::query;
use sqlx::query_as;
use sqlx::Pool;
use sqlx::Sqlite;
use tracing::debug;
use tracing::info;
use tracing::warn;
use utoipa::ToSchema;

#[derive(Deserialize, Clone, ToSchema)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct LoginPageForm {
    failure: Option<bool>,
}

pub async fn loginpage(
    State(AppState {
        pool: _, engine, ..
    }): State<AppState>,
    auth: AuthSession<Backend>,
    Form(form): Form<LoginPageForm>,
) -> Result<impl IntoResponse, Infallible> {
    let logged_in = auth.user.is_some();
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);

    let data = serde_json::json!({
    "git_ver": git_version!(),
        "title": "CZ4R Login",
        "admin": admin,
        "logged_in": logged_in,
        "failure": form.failure == Some(true)
    });

    Ok(RenderHtml("login.hbs", engine, data))
}

/// The outcome of attempting to log in. `must_change_pw` is `true` when the
/// credentials were valid but the account is required to change its
/// password before a session (or, for the API, a token) can be issued. On
/// the web this always redirects rather than returning this body; the API
/// returns it directly.
///
/// `token` is only ever populated by the JSON API (`POST /api/v1/login`):
/// it's the bearer token to send as `Authorization: Bearer <token>` on
/// every subsequent `/api/v1/*` or `/admin/api/v1/*` request. The HTML UI
/// doesn't use tokens at all - it authenticates via a cookie session
/// instead, so `token` is always `None` there.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LoginOutput {
    pub id: i64,
    pub username: String,
    pub must_change_pw: bool,
    pub token: Option<String>,
}

/// The result of validating a set of credentials, before any session/token
/// has actually been established.
enum AuthOutcome {
    /// The credentials were valid, but the account must change its
    /// password before it can be used to log in.
    MustChangePassword { id: i64, username: String },
    /// The credentials were fully valid.
    Success(Worker),
}

/// Shared credential-checking logic. Deliberately does *not* establish a
/// session or issue a token - that's transport-specific (a cookie session
/// for the web, a bearer token for the API), and is handled by `login` and
/// `login_api` respectively once this returns success.
async fn authenticate_core(pool: &Pool<Sqlite>, creds: LoginForm) -> Result<AuthOutcome, CustomError> {
    let LoginForm { username, password } = creds;

    let unauthorized = || {
        CustomError::new(
            anyhow!("Invalid username or password"),
            StatusCode::UNAUTHORIZED,
        )
    };

    let worker = query_as!(Worker, "select * from users where name = $1", username)
        .fetch_one(pool)
        .await;

    let worker = if let Ok(w) = worker {
        w
    } else {
        debug!(
            "user {} can't log in because there is nobody with that name in the database",
            &username
        );
        return Err(unauthorized());
    };

    if worker.deactivated {
        debug!(
            "user {} (id {}) can't log in because they are deactivated",
            &worker.name, &worker.id
        );
        return Err(unauthorized());
    }

    // Newly-created workers have an empty hash/salt and `must_change_pw =
    // true`, since they haven't set a password yet. Check this *before*
    // attempting to validate the (nonexistent) salt/password, so brand-new
    // accounts can reach the change-password flow instead of being rejected
    // as having an "invalid salt".
    if worker.must_change_pw {
        debug!(
            "user {} (id {}) has to change their password",
            &worker.name, &worker.id
        );
        return Ok(AuthOutcome::MustChangePassword {
            id: worker.id,
            username: worker.name,
        });
    }

    let salt = &worker.salt;
    let saltstr = SaltString::from_b64(salt.as_str());
    let saltstr = if let Ok(s) = saltstr {
        s
    } else {
        warn!(
            "user {} (id {}) can't log in because they have an invalid salt",
            &worker.name, &worker.id
        );
        return Err(unauthorized());
    };

    let hash = Scrypt
        .hash_password(password.as_bytes(), saltstr.as_salt())
        .unwrap()
        .to_string();

    if worker.hash != hash {
        debug!(
            "user {} (id {}) can't log in because they used the wrong password",
            &worker.name, &worker.id
        );
        return Err(unauthorized());
    }

    Ok(AuthOutcome::Success(worker))
}

/// Clear the `logged_out` flag for a freshly-authenticated worker. Shared
/// by both the web and API login handlers.
async fn mark_logged_in(pool: &Pool<Sqlite>, worker_id: i64) -> Result<(), CustomError> {
    query!(
        r#"
        update users
            set logged_out = 0
            where id = $1
        "#,
        worker_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// `POST /web/v1/login` — HTML-facing endpoint. Authenticates via
/// `authenticate_core`, then (on success) establishes a cookie session with
/// `auth.login`. Always responds with a redirect: to `/` on success, to the
/// change-password page when required, or back to the login page (with
/// `?failure=true`) on any error.
pub(crate) async fn login(
    mut auth: AuthSession<Backend>,
    State(AppState {
        pool, engine: _, ..
    }): State<AppState>,
    Form(login_form): Form<LoginForm>,
) -> Redirect {
    match authenticate_core(&pool, login_form).await {
        Ok(AuthOutcome::MustChangePassword { id, .. }) => {
            Redirect::to(format!("/change-pw?id={}", id).as_str())
        }
        Ok(AuthOutcome::Success(worker)) => {
            if auth.login(&worker).await.is_err() {
                warn!(
                    "user {} (id {}) authenticated but the session could not be started",
                    worker.name, worker.id
                );
                return Redirect::to("/loginpage?failure=true");
            }

            if mark_logged_in(&pool, worker.id).await.is_err() {
                return Redirect::to("/loginpage?failure=true");
            }

            info!("user {} (id {}) has logged in", worker.name, worker.id);
            Redirect::to("/")
        }
        Err(_) => Redirect::to("/loginpage?failure=true"),
    }
}

/// `POST /api/v1/login` — REST/JSON endpoint. Authenticates via
/// `authenticate_core`, then (on success) issues a bearer token instead of
/// a cookie session - this endpoint doesn't touch `AuthSession` at all.
#[utoipa::path(
    post,
    path = "/api/v1/login",
    request_body = LoginForm,
    responses((status = OK, body = LoginOutput)),
    tag = super::USER_TAG
)]
pub(crate) async fn login_api(
    State(AppState { pool, .. }): State<AppState>,
    Json(login_form): Json<LoginForm>,
) -> Result<Json<LoginOutput>, ApiError> {
    match authenticate_core(&pool, login_form).await.map_err(ApiError::from)? {
        AuthOutcome::MustChangePassword { id, username } => Ok(Json(LoginOutput {
            id,
            username,
            must_change_pw: true,
            token: None,
        })),
        AuthOutcome::Success(worker) => {
            let token = api_auth::issue_token(&pool, worker.id)
                .await
                .map_err(ApiError::from)?;
            mark_logged_in(&pool, worker.id)
                .await
                .map_err(ApiError::from)?;

            info!(
                "user {} (id {}) has logged in (api token issued)",
                worker.name, worker.id
            );

            Ok(Json(LoginOutput {
                id: worker.id,
                username: worker.name,
                must_change_pw: false,
                token: Some(token),
            }))
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema)]
pub struct LogoutForm {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LogoutUserOutput {
    pub id: i64,
}

/// Forcibly log another worker out, by flagging their account
/// `logged_out`. This invalidates both their cookie session (checked by
/// `Backend::get_user`) and any bearer tokens they hold (checked by
/// [`crate::api_auth::ApiAuth`]) - one flag, one mechanism, both transports.
async fn logout_user_core(
    pool: &Pool<Sqlite>,
    user: Option<&crate::CurrentUser>,
    input: LogoutForm,
) -> Result<LogoutUserOutput, CustomError> {
    get_admin(user)?;

    query!(
        r#"
        update users
            set logged_out = 1
            where id = $1
        "#,
        input.id
    )
    .execute(pool)
    .await?;

    Ok(LogoutUserOutput { id: input.id })
}

/// `POST /admin/web/v1/logout-worker` — HTML-facing endpoint. Forcibly logs
/// another worker's session out.
pub async fn logout_user(
    State(AppState { pool, .. }): State<AppState>,
    auth: AuthSession<Backend>,
    Form(form): Form<LogoutForm>,
) -> Result<impl IntoResponse, CustomError> {
    logout_user_core(&pool, current_user(&auth).as_ref(), form).await?;
    Ok(StatusCode::OK)
}

/// `POST /admin/api/v1/logout-worker` — REST/JSON endpoint.
#[utoipa::path(
    post,
    path = "/admin/api/v1/logout-worker",
    request_body = LogoutForm,
    responses((status = OK, body = LogoutUserOutput)),
    security(("bearer_auth" = [])),
    tag = super::ADMIN_TAG
)]
pub async fn logout_user_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
    Json(form): Json<LogoutForm>,
) -> Result<Json<LogoutUserOutput>, ApiError> {
    to_api(logout_user_core(&pool, Some(&user), form).await)
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LogoutOutput {
    pub logged_out: bool,
}

/// `POST /web/v1/logout` — HTML-facing endpoint. Ends the cookie session.
pub(crate) async fn logout(mut auth: AuthSession<Backend>) -> Redirect {
    if let Some(user) = auth.user.clone() {
        if auth.logout().await.is_ok() {
            debug!("user {} (id {}) logged out", user.name, user.id);
            return Redirect::to("/");
        }
        warn!("user {} (id {}) could not log out", user.name, user.id);
    } else {
        debug!("user who is not logged in attempted to log out");
    }
    Redirect::to("/loginpage?failure=true")
}

/// `POST /api/v1/logout` — REST/JSON endpoint. Revokes the bearer token
/// used to authenticate this very request.
#[utoipa::path(
    post,
    path = "/api/v1/logout",
    responses((status = OK, body = LogoutOutput)),
    security(("bearer_auth" = [])),
    tag = super::USER_TAG
)]
pub(crate) async fn logout_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, token }: ApiAuth,
) -> Result<Json<LogoutOutput>, ApiError> {
    api_auth::revoke_token(&pool, &token)
        .await
        .map_err(ApiError::from)?;
    debug!("user {} (id {}) revoked their api token", user.name, user.id);
    Ok(Json(LogoutOutput { logged_out: true }))
}
