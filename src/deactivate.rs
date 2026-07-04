use crate::api_auth::ApiAuth;
use crate::errors::{to_api, ApiError, CustomError};
use crate::{current_user, get_admin};
use crate::AppState;

use super::Worker;

use anyhow::anyhow;
use axum::response::IntoResponse;
use axum::response::Redirect;
use sqlx::query;

use crate::Backend;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Form;
use axum::Json;
use axum_login::AuthSession;
use serde::{Deserialize, Serialize};
use sqlx::Pool;
use tracing::debug;
use tracing::info;
use utoipa::ToSchema;

/// Request body shared by the web form and the JSON API for deactivating a
/// worker.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub(crate) struct DeactivateInput {
    pub user: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct DeactivateOutput {
    pub user: i64,
}

async fn deactivate_core(
    pool: &Pool<sqlx::Sqlite>,
    user: Option<&crate::CurrentUser>,
    input: DeactivateInput,
) -> Result<DeactivateOutput, CustomError> {
    let (my_id, my_name) = get_admin(user)?;
    if input.user == my_id {
        debug!(
            "admin {} (id {}) tried to deactivate themself",
            my_name, my_id
        );
        return Err(CustomError::new(
            anyhow!("A user cannot deactivate themselves"),
            StatusCode::FORBIDDEN,
        ));
    }

    query!(
        "update users set deactivated = true where id = $1;",
        input.user
    )
    .execute(pool)
    .await?;

    info!("admin {} deactivated user {}", my_id, input.user);

    Ok(DeactivateOutput { user: input.user })
}

/// `POST /admin/web/v1/deactivate-worker` — HTML-facing endpoint.
pub(crate) async fn deactivate(
    auth: AuthSession<Backend>,
    State(AppState { pool, .. }): State<AppState>,
    Form(input): Form<DeactivateInput>,
) -> Result<impl IntoResponse, CustomError> {
    deactivate_core(&pool, current_user(&auth).as_ref(), input).await?;
    Ok(Redirect::to("/admin/worker-edit"))
}

/// `POST /admin/api/v1/deactivate-worker` — REST/JSON endpoint.
/// Admins are not capable of deactivating themselves, and the API will return a 403 error if they try.
#[utoipa::path(
    post,
    path = "/admin/api/v1/deactivate-worker",
    request_body = DeactivateInput,
    responses((status = OK, body = DeactivateOutput)),
    security(("bearer_auth" = [])),
    tag = super::ADMIN_TAG
)]
pub(crate) async fn deactivate_api(
    ApiAuth { user, .. }: ApiAuth,
    State(AppState { pool, .. }): State<AppState>,
    Json(input): Json<DeactivateInput>,
) -> Result<Json<DeactivateOutput>, ApiError> {
    to_api(deactivate_core(&pool, Some(&user), input).await)
}
