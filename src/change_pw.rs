//Name=&Address=&Phone=&Email=&Hourly=&Mileage=&Drivetime=

use crate::errors::{to_api, ApiError, CustomError};
use crate::get_user;
use crate::AppState;
use crate::Backend;
use anyhow::anyhow;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::Form;
use axum::Json;
use axum_login::AuthSession;
use axum_template::RenderHtml;
use git_version::git_version;
use scrypt::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Scrypt,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::query;
use sqlx::Pool;
use tracing::*;
use utoipa::ToSchema;

#[derive(Deserialize)]
pub(crate) struct ChangePwPageForm {
    id: Option<i64>,
    no_match: Option<bool>,
}

pub(crate) async fn change_pw_page(
    State(AppState {
        pool: _, engine, ..
    }): State<AppState>,
    mut _auth: AuthSession<Backend>, //never logged in
    Form(form): Form<ChangePwPageForm>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    let id = if let Some(id) = form.id {
        id
    } else {
        return Err(CustomError::new(
            anyhow!("No ID selected."),
            StatusCode::BAD_REQUEST,
        ));
    };

    let data = json!({
    "git_ver": git_version!(),
        "title": "CZ4R Login",
        "admin": false,
        "logged_in": true,
        "failure": form.no_match == Some(true),
        "chg_id": id
    });

    Ok(RenderHtml("changepw.hbs", engine, data))
}

/// Request body shared by the web form and the JSON API for changing a
/// password.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub(crate) struct ChangePwInput {
    pub id: i64,
    pub password1: String,
    pub password2: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ChangePwOutput {
    pub id: i64,
    pub changed: bool,
}

/// Shared business logic. Returns `Ok(ChangePwOutput { changed: false, .. })`
/// (rather than an error) when the two passwords didn't match, since that's
/// a normal, expected outcome rather than a failure.
async fn change_pw_core(
    pool: &Pool<sqlx::Sqlite>,
    input: &ChangePwInput,
) -> Result<ChangePwOutput, CustomError> {
    let must_change = query!(
        r#"
    select (must_change_pw) from users 
    where id = $1
    and users.deactivated = false;
    "#,
        input.id
    )
    .fetch_one(pool)
    .await?
    .must_change_pw;

    if !must_change {
        info!(
            "user id {} attempted to change their password when not allowed to",
            input.id
        );
        return Err(CustomError::new(
            anyhow!(
                "User {} cannot change their password right now. Nice try.",
                input.id
            ),
            StatusCode::FORBIDDEN,
        ));
    }

    if input.password1 != input.password2 {
        debug!("user id {} put in the wrong password", input.id);
        return Ok(ChangePwOutput {
            id: input.id,
            changed: false,
        });
    }

    let salt = SaltString::generate(&mut scrypt::password_hash::rand_core::OsRng);

    let hash = Scrypt
        .hash_password(input.password1.as_bytes(), salt.as_salt())
        .unwrap()
        .to_string();

    let salt = salt.as_str();

    query!(
        r#"
    update users
    set
    hash = $1,
    salt = $2,
    must_change_pw = false
    where id = $3;"#,
        hash,
        salt,
        input.id
    )
    .execute(pool)
    .await?;

    info!("user id {} changed their password", input.id);

    Ok(ChangePwOutput {
        id: input.id,
        changed: true,
    })
}

/// `POST /web/v1/change-pw` — HTML-facing endpoint, redirects back to the
/// change-password page (with `no_match=true` on failure) or to the login
/// page on success.
pub(crate) async fn change_pw(
    State(AppState { pool, .. }): State<AppState>,
    mut _auth: AuthSession<Backend>,
    Form(form): Form<ChangePwInput>,
) -> Result<impl IntoResponse, CustomError> {
    let out = change_pw_core(&pool, &form).await?;

    if !out.changed {
        return Ok(Redirect::to(&format!(
            "/change-pw?id={}&no_match=true",
            out.id
        )));
    }

    Ok(Redirect::to("/loginpage"))
}

/// `POST /api/v1/change-pw` — REST/JSON endpoint.
#[utoipa::path(
    post,
    path = "/api/v1/change-pw",
    request_body = ChangePwInput,
    responses((status = OK, body = ChangePwOutput)),
    tag = super::USER_TAG
)]
pub(crate) async fn change_pw_api(
    State(AppState { pool, .. }): State<AppState>,
    mut _auth: AuthSession<Backend>,
    Json(input): Json<ChangePwInput>,
) -> Result<Json<ChangePwOutput>, ApiError> {
    to_api(change_pw_core(&pool, &input).await)
}
