//Name=&Address=&Phone=&Email=&Hourly=&Mileage=&Drivetime=

use super::Worker;
use crate::errors::CustomError;
use crate::get_admin;
use crate::AppState;
use crate::Backend;
use anyhow::{bail, anyhow};
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
        get_admin(auth)?;

        let hourly = Decimal::from_str_exact(&workerdata.Hourly) ? * Decimal::ONE_HUNDRED;

        let mileage = Decimal::from_str_exact(&workerdata.Mileage)? * Decimal::ONE_HUNDRED;

        let drivetime = Decimal::from_str_exact(&workerdata.Drivetime)? * Decimal::ONE_HUNDRED;

        let flatrate = Decimal::from_str_exact(&workerdata.Flatrate)? * Decimal::ONE_HUNDRED;


        let admin = match workerdata.Admin.as_deref() {
            Some("on" | "true" | "yes") => true,
            Some("off" | "false" | "no") | None => false,
            _ => return Err(CustomError(anyhow!("Client didn't return a boolean string"))),
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
        ).fetch_one(&pool).await?.id;


        Ok(Redirect::to(
            format!("/admin/worker-edit?worker={}", id).as_str(),
        ))

}
