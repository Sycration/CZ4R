
use crate::api_auth::ApiAuth;
use crate::errors::{to_api, ApiError, CustomError};
use crate::workeredit::WorkerSummary;
use crate::{Worker, current_user, get_admin, get_user};
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
use rust_decimal::prelude::*;
use serde::{Deserialize, Serialize};
use sqlx::{query, query_as};
use sqlx::Pool;
use utoipa::ToSchema;


/// Gets the current user's details
/// Equivalent to the `/admin/api/v1/users/:id` endpoint, but for the currently logged-in user.
#[utoipa::path(
    get,
    path = "/api/v1/whoami",
    responses((status = OK, body = WorkerSummary)),
    security(("bearer_auth" = [])),
    tag = super::USER_TAG,
)]
pub(crate) async fn whoami(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth
) -> Result<Json<WorkerSummary>, ApiError> {
    let me = get_user(Some(&user))?;
        let worker = query_as!(Worker, "select * from users where id = $1;", me.0)
        .fetch_optional(&pool)
        .await?
        .ok_or_else(|| {
            CustomError::new(anyhow!("No user with id {0}", me.0), StatusCode::NOT_FOUND)
        })?;

    to_api(Ok(WorkerSummary::from(worker)))
}
