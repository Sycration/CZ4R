//Name=&Address=&Phone=&Email=&Hourly=&Mileage=&Drivetime=

use crate::api_auth::ApiAuth;
use crate::errors::{to_api, ApiError, CustomError};
use crate::{current_user, get_admin};
use crate::AppState;
use crate::Backend;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::Form;
use axum::Json;
use axum_login::AuthSession;
use serde::{Deserialize, Serialize};
use sqlx::query;
use sqlx::Pool;
use tracing::info;
use utoipa::ToSchema;

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub(crate) struct ResetPwInput {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ResetPwOutput {
    pub id: i64,
}

async fn reset_pw_core(
    pool: &Pool<sqlx::Sqlite>,
    user: Option<&crate::CurrentUser>,
    input: ResetPwInput,
) -> Result<ResetPwOutput, CustomError> {
    let (my_id, my_name) = get_admin(user)?;
    query!(
        r#"
        update users
        set must_change_pw = true
        where id = $1
        "#,
        input.id
    )
    .execute(pool)
    .await?;

    info!(
        "admin {my_name} (id {my_id}) reset user {}'s password",
        input.id
    );

    Ok(ResetPwOutput { id: input.id })
}

/// `POST /admin/web/v1/reset-pw` — HTML-facing endpoint.
pub(crate) async fn reset_pw(
    State(AppState { pool, .. }): State<AppState>,
    auth: AuthSession<Backend>,
    Form(input): Form<ResetPwInput>,
) -> Result<impl IntoResponse, CustomError> {
    let out = reset_pw_core(&pool, current_user(&auth).as_ref(), input).await?;

    Ok(Redirect::to(
        format!("/admin/worker-edit?worker={}", out.id).as_str(),
    ))
}

/// Forces a user to change their password on next login.
#[utoipa::path(
    post,
    path = "/admin/api/v1/reset-pw",
    request_body = ResetPwInput,
    responses((status = OK, body = ResetPwOutput)),
    security(("bearer_auth" = [])),
    tag = super::ADMIN_TAG
)]
pub(crate) async fn reset_pw_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
    Json(input): Json<ResetPwInput>,
) -> Result<Json<ResetPwOutput>, ApiError> {
    to_api(reset_pw_core(&pool, Some(&user), input).await)
}
