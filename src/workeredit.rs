use super::Worker;
use crate::api_auth::ApiAuth;
use crate::current_user;
use crate::errors::{to_api, ApiError, CustomError};
use crate::get_admin;
use crate::AppState;
use crate::Backend;
use crate::IntoResponse;
use anyhow::anyhow;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum::{Form, Json};
use axum_login::AuthSession;
use axum_template::RenderHtml;
use git_version::git_version;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{query_as, Pool};
use utoipa::ToSchema;

#[derive(Deserialize)]
pub(crate) struct WorkerEditForm {
    worker: Option<i64>,
    creating: Option<bool>,
}

/// A worker's admin-visible profile, safe to expose over the JSON API -
/// unlike [`Worker`], this deliberately excludes `hash`/`salt`.
///
/// This is the shape used both by `/admin/api/v1/users` (worker
/// management) and, filtered to active workers, by job-assignment flows
/// that just need to know who exists and pick some subset of them.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct WorkerSummary {
    pub id: i64,
    pub name: String,
    pub admin: bool,
    pub address: String,
    pub phone: String,
    pub email: String,
    pub rate_hourly_cents: i64,
    pub rate_mileage_cents: i64,
    pub rate_drive_hourly_cents: i64,
    pub flat_rate_cents: i64,
    pub must_change_pw: bool,
    pub deactivated: bool,
}

impl From<Worker> for WorkerSummary {
    fn from(w: Worker) -> Self {
        Self {
            id: w.id,
            name: w.name,
            admin: w.admin,
            address: w.address,
            phone: w.phone,
            email: w.email,
            rate_hourly_cents: w.rate_hourly_cents,
            rate_mileage_cents: w.rate_mileage_cents,
            rate_drive_hourly_cents: w.rate_drive_hourly_cents,
            flat_rate_cents: w.flat_rate_cents,
            must_change_pw: w.must_change_pw,
            deactivated: w.deactivated,
        }
    }
}

async fn list_users_core(
    pool: &Pool<sqlx::Sqlite>,
    user: Option<&crate::CurrentUser>,
) -> Result<Vec<WorkerSummary>, CustomError> {
    get_admin(user)?;

    let users = query_as!(Worker, "select * from users order by id asc;")
        .fetch_all(pool)
        .await?;

    Ok(users.into_iter().map(WorkerSummary::from).collect())
}

async fn get_user_core(
    pool: &Pool<sqlx::Sqlite>,
    user: Option<&crate::CurrentUser>,
    id: i64,
) -> Result<WorkerSummary, CustomError> {
    get_admin(user)?;

    let worker = query_as!(Worker, "select * from users where id = $1;", id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| {
            CustomError::new(anyhow!("No user with id {id}"), StatusCode::NOT_FOUND)
        })?;

    Ok(WorkerSummary::from(worker))
}

/// The full list of workers (active and deactivated), for worker management and for
/// picking assignees when creating or editing a job.
#[utoipa::path(
    get,
    path = "/admin/api/v1/users",
    responses((status = OK, body = Vec<WorkerSummary>)),
    security(("bearer_auth" = [])),
    tag = super::ADMIN_TAG
)]
pub(crate) async fn list_users_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
) -> Result<Json<Vec<WorkerSummary>>, ApiError> {
    to_api(list_users_core(&pool, Some(&user)).await)
}

/// Gets a single worker's profile, e.g. to prefill a worker-edit form.
#[utoipa::path(
    get,
    path = "/admin/api/v1/users/{id}",
    params(("id" = i64, Path, description = "Worker id")),
    responses((status = OK, body = WorkerSummary)),
    security(("bearer_auth" = [])),
    tag = super::ADMIN_TAG
)]
pub(crate) async fn get_user_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
    Path(id): Path<i64>,
) -> Result<Json<WorkerSummary>, ApiError> {
    to_api(get_user_core(&pool, Some(&user), id).await)
}

pub(crate) async fn workeredit(
    State(AppState { pool, engine, .. }): State<AppState>,
    auth: AuthSession<Backend>,
    Form(worker): Form<WorkerEditForm>,
) -> Result<impl IntoResponse, CustomError> {
    let id = get_admin(current_user(&auth).as_ref())?;

    let users = sqlx::query_as!(
        Worker,
        "
        select * from users where deactivated = false order by id asc;
        "
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let mut selectlist = users
        .iter()
        .map(|w| (w.id, w.name.as_str()))
        .collect::<Vec<_>>();

    let data = serde_json::json!({
    "git_ver": git_version!(),
        "admin": true,
        "logged_in": true,
        "title": "CZ4R Worker Edit",
        "target": "worker-edit",
        "creating": worker.creating == Some(true),
        "selected": worker.worker,
        "selectlist": selectlist,
        "own_id": id,
        "workerlist": (users.iter().map(|u|json!({
            "id": u.id,
            "name": u.name,
            "hash": u.hash,
            "salt": u.salt,
            "admin": u.admin,
            "address": u.address,
            "phone": u.phone,
            "email": u.email,
            "rate_hourly_cents": format!("{:.2}", (u.rate_hourly_cents as f64 / 100.)),
            "rate_mileage_cents": format!("{:.2}", (u.rate_mileage_cents as f64 / 100.)),
            "rate_drive_hourly_cents": format!("{:.2}", (u.rate_drive_hourly_cents as f64 / 100.)),
            "flat_rate_cents": format!("{:.2}", (u.flat_rate_cents as f64 / 100.)),
            "must_change_pw": u.must_change_pw
        })).collect::<Vec<_>>())
    });

    Ok(RenderHtml("workeredit.hbs", engine, data))
}
