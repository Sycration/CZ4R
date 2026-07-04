use crate::api_auth::ApiAuth;
use crate::errors::{to_api, ApiError, CustomError};
use crate::{current_user, get_admin};
use crate::AppState;

use super::Worker;

use crate::Backend;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::Form;
use axum::Json;
use axum_login::AuthSession;
use axum_template::RenderHtml;
use git_version::git_version;
use serde::{Deserialize, Serialize};
use sqlx::query;
use sqlx::query_as;
use sqlx::Pool;
use tracing::info;
use utoipa::ToSchema;

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct RestoreInput {
    pub user: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RestoreOutput {
    pub user: i64,
}

#[derive(Serialize, Deserialize)]
struct RestoreListItem {
    id: i64,
    name: String,
}

pub async fn restorepage(
    State(AppState { pool, engine, .. }): State<AppState>,
    auth: AuthSession<Backend>,
) -> Result<impl IntoResponse, CustomError> {
    get_admin(current_user(&auth).as_ref())?;

    let workers = query_as!(
        RestoreListItem,
        "select id, name from users where users.deactivated = true order by id asc"
    )
    .fetch_all(&pool)
    .await?;

    let data = serde_json::json!({
    "git_ver": git_version!(),
        "title": "CZ4R Restore Workers",
        "admin": true,
        "logged_in": true,
        "workers": workers
    });

    Ok(RenderHtml("restore.hbs", engine, data))
}

async fn restore_core(
    pool: &Pool<sqlx::Sqlite>,
    user: Option<&crate::CurrentUser>,
    input: RestoreInput,
) -> Result<RestoreOutput, CustomError> {
    let (my_id, my_name) = get_admin(user)?;

    query!(
        "update users set deactivated = false where id = $1",
        input.user
    )
    .execute(pool)
    .await?;

    info!(
        "admin {my_name} (id {my_id}) restored deactivated user {}",
        input.user
    );

    Ok(RestoreOutput { user: input.user })
}

/// `POST /admin/web/v1/restore-worker` — HTML-facing endpoint.
pub(crate) async fn restore(
    auth: AuthSession<Backend>,
    State(AppState { pool, .. }): State<AppState>,
    Form(input): Form<RestoreInput>,
) -> Result<impl IntoResponse, CustomError> {
    restore_core(&pool, current_user(&auth).as_ref(), input).await?;
    Ok(Redirect::to("/admin/restore"))
}

/// `POST /admin/api/v1/restore-worker` — REST/JSON endpoint.
#[utoipa::path(
    post,
    path = "/admin/api/v1/restore-worker",
    request_body = RestoreInput,
    responses((status = OK, body = RestoreOutput)),
    security(("bearer_auth" = [])),
    tag = super::ADMIN_TAG
)]
pub(crate) async fn restore_api(
    ApiAuth { user, .. }: ApiAuth,
    State(AppState { pool, .. }): State<AppState>,
    Json(input): Json<RestoreInput>,
) -> Result<Json<RestoreOutput>, ApiError> {
    to_api(restore_core(&pool, Some(&user), input).await)
}
