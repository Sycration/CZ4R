//Name=&Address=&Phone=&Email=&Hourly=&Mileage=&Drivetime=

use super::Worker;
use crate::api_auth::ApiAuth;
use crate::errors::{to_api, ApiError, CustomError};
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

/// Request body shared by the web form and the JSON API for changing a
/// worker's details.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub(crate) struct WorkerChangeInput {
    pub id: i64,
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

/// The legacy, PascalCase-named form fields the HTML client posts.
#[derive(Deserialize)]
pub(crate) struct WorkerChangeForm {
    Name: String,
    Address: String,
    Phone: String,
    Email: String,
    Hourly: String,
    Mileage: String,
    Drivetime: String,
    Flatrate: String,
    Admin: Option<String>,
    id: i64,
}

impl TryFrom<WorkerChangeForm> for WorkerChangeInput {
    type Error = CustomError;

    fn try_from(form: WorkerChangeForm) -> Result<Self, Self::Error> {
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
            id: form.id,
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
pub(crate) struct WorkerChangeOutput {
    pub id: i64,
}

async fn change_worker_core(
    pool: &Pool<sqlx::Sqlite>,
    user: Option<&crate::CurrentUser>,
    input: WorkerChangeInput,
) -> Result<WorkerChangeOutput, CustomError> {
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

    if my_id == input.id && !admin {
        return Err(CustomError::new(
            anyhow!("Admin cannot remove their own admin privileges"),
            StatusCode::FORBIDDEN,
        ));
    }

    query!(
        r#"update users 
            set 
            name = $1, 
            admin = $2, 
            address = $3, 
            phone = $4, 
            email = $5, 
            rate_hourly_cents = $6, 
            rate_mileage_cents = $7, 
            rate_drive_hourly_cents = $8,
            flat_rate_cents = $9
            where id = $10; 
        "#,
        input.name,
        admin,
        input.address,
        input.phone,
        input.email,
        hourly,
        mileage,
        drivetime,
        flatrate,
        input.id
    )
    .execute(pool)
    .await?;

    tracing::info!("admin {} (id {}) modified user {} as follows:\nname: {}\nadmin: {}\naddress: {}\nphone number: {}\nemail address: {}\nhourly rate (cents): {}\ndriving milage rate (cents): {}\ndriving hourly rate (cents): {}\nflat rate worker: {}",
        my_id,
        my_name,
        input.id,
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

    Ok(WorkerChangeOutput { id: input.id })
}

/// `POST /admin/web/v1/change-worker` — HTML-facing endpoint.
pub(crate) async fn change_worker(
    State(AppState { pool, .. }): State<AppState>,
    auth: AuthSession<Backend>,
    Form(form): Form<WorkerChangeForm>,
) -> Result<impl IntoResponse, CustomError> {
    let input = WorkerChangeInput::try_from(form)?;
    let out = change_worker_core(&pool, current_user(&auth).as_ref(), input).await?;

    Ok(Redirect::to(
        format!("/admin/worker-edit?worker={}", out.id).as_str(),
    ))
}

/// `POST /admin/api/v1/change-worker` — REST/JSON endpoint.
#[utoipa::path(
    post,
    path = "/admin/api/v1/change-worker",
    request_body = WorkerChangeInput,
    responses((status = OK, body = WorkerChangeOutput)),
    security(("bearer_auth" = [])),
    tag = super::ADMIN_TAG
)]
pub(crate) async fn change_worker_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
    Json(input): Json<WorkerChangeInput>,
) -> Result<Json<WorkerChangeOutput>, ApiError> {
    to_api(change_worker_core(&pool, Some(&user), input).await)
}
