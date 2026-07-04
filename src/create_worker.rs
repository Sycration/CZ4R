//Name=&Address=&Phone=&Email=&Hourly=&Mileage=&Drivetime=

use super::Worker;
use crate::api_auth::ApiAuth;
use crate::errors::{to_api_with_status, ApiError, CustomError};
use crate::{current_user, get_admin};
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
use sqlx::query;
use sqlx::Pool;
use utoipa::ToSchema;

/// Request body shared by the web form and the JSON API for creating a
/// worker. Money fields are decimal strings (e.g. `"12.50"`), matching what
/// the HTML form sends; the JSON API accepts the same convention for
/// consistency between the two.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub(crate) struct WorkerCreateInput {
    pub name: String,
    pub address: String,
    pub phone: String,
    pub email: String,
    pub hourly: String,
    pub mileage: String,
    pub drivetime: String,
    pub flatrate: String,
    pub admin: Option<bool>,
}

/// The legacy, PascalCase-named form fields the HTML client posts, where
/// `Admin` is a loosely-typed "on/true/yes" style string rather than a real
/// boolean.
#[derive(Deserialize)]
pub(crate) struct WorkerCreateForm {
    Name: String,
    Address: String,
    Phone: String,
    Email: String,
    Hourly: String,
    Mileage: String,
    Drivetime: String,
    Flatrate: String,
    Admin: Option<String>,
}

impl TryFrom<WorkerCreateForm> for WorkerCreateInput {
    type Error = CustomError;

    fn try_from(form: WorkerCreateForm) -> Result<Self, Self::Error> {
        let admin = match form.Admin.as_deref() {
            Some("on" | "true" | "yes") => Some(true),
            Some("off" | "false" | "no") | None => Some(false),
            _ => {
                return Err(CustomError::new(
                    anyhow!("Client didn't return a boolean string"),
                    StatusCode::BAD_REQUEST,
                ))
            }
        };

        Ok(Self {
            name: form.Name,
            address: form.Address,
            phone: form.Phone,
            email: form.Email,
            hourly: form.Hourly,
            mileage: form.Mileage,
            drivetime: form.Drivetime,
            flatrate: form.Flatrate,
            admin,
        })
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct WorkerCreateOutput {
    pub id: i64,
}

/// Shared business logic for creating a worker.
async fn create_worker_core(
    pool: &Pool<sqlx::Sqlite>,
    user: Option<&crate::CurrentUser>,
    input: WorkerCreateInput,
) -> Result<WorkerCreateOutput, CustomError> {
    let (my_id, my_name) = get_admin(user)?;

    let hourly = Decimal::from_str_exact(&input.hourly)? * Decimal::ONE_HUNDRED;
    let mileage = Decimal::from_str_exact(&input.mileage)? * Decimal::ONE_HUNDRED;
    let drivetime = Decimal::from_str_exact(&input.drivetime)? * Decimal::ONE_HUNDRED;
    let flatrate = Decimal::from_str_exact(&input.flatrate)? * Decimal::ONE_HUNDRED;

    let admin = input.admin.unwrap_or(false);

    let hourly = hourly.to_i32().unwrap();
    let mileage = mileage.to_i32().unwrap();
    let drivetime = drivetime.to_i32().unwrap();
    let flatrate = flatrate.to_i32().unwrap();
    let id = query!(
            r#"insert into users (name, hash, salt, admin, address, phone, email, rate_hourly_cents, rate_mileage_cents, rate_drive_hourly_cents, flat_rate_cents, must_change_pw)
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        returning id;
        "#,
            input.name,
            "",
            "",
            admin,
            input.address,
            input.phone,
            input.email,
            hourly,
            mileage,
            drivetime,
            flatrate,
            true
        ).fetch_one(pool).await?.id;

    tracing::info!("admin {} (id {}) created new user {} as follows:\nname: {}\nadmin: {}\naddress: {}\nphone number: {}\nemail address: {}\nhourly rate (cents): {}\ndriving milage rate (cents): {}\ndriving hourly rate (cents): {}\nflat rate worker: {}",
        my_name,
        my_id,
        id,
        input.name,
        admin,
        input.address,
        input.phone,
        input.email,
        hourly,
        mileage,
        drivetime,
        flatrate,
    );

    Ok(WorkerCreateOutput { id })
}

/// `POST /admin/web/v1/create-worker` — HTML-facing endpoint, redirects to
/// the newly created worker's edit page.
pub(crate) async fn create_worker(
    State(AppState { pool, .. }): State<AppState>,
    auth: AuthSession<Backend>,
    Form(form): Form<WorkerCreateForm>,
) -> Result<impl IntoResponse, CustomError> {
    let input = WorkerCreateInput::try_from(form)?;
    let out = create_worker_core(&pool, current_user(&auth).as_ref(), input).await?;

    Ok(Redirect::to(
        format!("/admin/worker-edit?worker={}", out.id).as_str(),
    ))
}

/// Responds `201 Created` with the new worker's id on success.
#[utoipa::path(
    post,
    path = "/admin/api/v1/create-worker",
    request_body = WorkerCreateInput,
    responses((status = CREATED, body = WorkerCreateOutput)),
    security(("bearer_auth" = [])),
    tag = super::ADMIN_TAG
)]
pub(crate) async fn create_worker_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
    Json(input): Json<WorkerCreateInput>,
) -> Result<(StatusCode, Json<WorkerCreateOutput>), ApiError> {
    to_api_with_status(
        create_worker_core(&pool, Some(&user), input).await,
        StatusCode::CREATED,
    )
}
