//Name=&Address=&Phone=&Email=&Hourly=&Mileage=&Drivetime=

use super::Worker;
use crate::errors::CustomError;
use crate::AppState;
use crate::Backend;
use axum::debug_handler;
use axum::extract::Path;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::Form;
use axum_login::AuthSession;
use rust_decimal::prelude::*;
use serde::Deserialize;
use sqlx::query;
use sqlx::query_as;
use sqlx::Pool;
use sqlx::Postgres;

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

pub(crate) async fn create_worker(
State(AppState { pool, engine }): State<AppState>,
    //Path(id): Path<i64>,
    mut auth: AuthSession<Backend>,
    Form(workerdata): Form<WorkerCreateForm>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    if let Some(true) = auth.user.map(|u| u.admin) {
        let hourly = Decimal::from_str_exact(&workerdata.Hourly);
        let hourly = if let Ok(v) = hourly {
            v * Decimal::ONE_HUNDRED
        } else {
            return Err(CustomError::Database(format!(
                "Nonsense data: {} is not a number",
                workerdata.Hourly
            )).build(&engine));
        };
        let mileage = Decimal::from_str_exact(&workerdata.Mileage);
        let mileage = if let Ok(v) = mileage {
            v * Decimal::ONE_HUNDRED
        } else {
            return Err(CustomError::Database(format!(
                "Nonsense data: {} is not a number",
                workerdata.Mileage
            )).build(&engine));
        };
        let drivetime = Decimal::from_str_exact(&workerdata.Drivetime);
        let drivetime = if let Ok(v) = drivetime {
            v * Decimal::ONE_HUNDRED
        } else {
            return Err(CustomError::Database(format!(
                "Nonsense data: {} is not a number",
                workerdata.Drivetime
            )).build(&engine));
        };
        let flatrate = Decimal::from_str_exact(&workerdata.Flatrate);
        let flatrate = if let Ok(v) = flatrate {
            v * Decimal::ONE_HUNDRED
        } else {
            return Err(CustomError::Database(format!(
                "Nonsense data: {} is not a number",
                workerdata.Flatrate
            )).build(&engine));
        };

        let admin = match workerdata.Admin.as_deref() {
            Some("on" | "true" | "yes") => true,
            Some("off" | "false" | "no") | None => false,
            _ => return Err(CustomError::Database("Not a boolean".to_string()).build(&engine)),
        };

        let id = query!(
            r#"insert into users (name, hash, salt, admin, address, phone, email, rate_hourly_cents, rate_mileage_cents, rate_drive_hourly_cents, flat_rate_cents, must_change_pw)
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        returning id;
        "#,
            workerdata.Name,
            "",
            "",
            admin,
            workerdata.Address,
            workerdata.Phone,
            workerdata.Email,
            hourly.to_i32().unwrap(),
            mileage.to_i32().unwrap(),
            drivetime.to_i32().unwrap(),
            flatrate.to_i32().unwrap(),
            true
        ).fetch_one(&pool).await;

        let id = if let Ok(id) = id {
            id.id
        } else {
            return Err(CustomError::Database(format!(
                "Nonsense data returned from database: {} is not a valid ID",
                id.unwrap_err()
            )).build(&engine));
        };

        Ok(Redirect::to(
            format!("/admin/worker-edit?worker={}", id).as_str(),
        ))
    } else {
        Err(CustomError::Auth("Not logged in as admin".to_string()).build(&engine))
    }
}
