//Name=&Address=&Phone=&Email=&Hourly=&Mileage=&Drivetime=

use super::Worker;
use crate::errors::CustomError;
use crate::get_admin;
use crate::AppState;
use anyhow::anyhow;
use crate::Backend;
use axum::debug_handler;
use axum::extract::Path;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::Form;
use axum_login::AuthSession;
use axum_template::engine;
use rust_decimal::prelude::*;
use serde::Deserialize;
use sqlx::query;
use sqlx::query_as;
use sqlx::Pool;
use sqlx::Postgres;

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

// Result<impl IntoResponse, impl IntoResponse>

pub(crate) async fn change_worker(
    State(AppState { pool, engine, .. }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(workerdata): Form<WorkerChangeForm>,
) -> Result<impl IntoResponse, impl IntoResponse> {
        get_admin(auth)?;
        let hourly = Decimal::from_str_exact(&workerdata.Hourly)? * Decimal::ONE_HUNDRED;

        let mileage = Decimal::from_str_exact(&workerdata.Mileage)? * Decimal::ONE_HUNDRED;

        let drivetime = Decimal::from_str_exact(&workerdata.Drivetime)? * Decimal::ONE_HUNDRED;
        let flatrate = Decimal::from_str_exact(&workerdata.Flatrate)? * Decimal::ONE_HUNDRED;

        let admin = match workerdata.Admin.as_deref() {
            Some("on" | "true" | "yes") => true,
            Some("off" | "false" | "no") | None => false,
            _ => return Err(CustomError(anyhow!("Client didn't return a boolean string"))),
        };
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
            workerdata.Name,
            admin,
            workerdata.Address,
            workerdata.Phone,
            workerdata.Email,
            hourly.to_i32().unwrap(),
            mileage.to_i32().unwrap(),
            drivetime.to_i32().unwrap(),
            flatrate.to_i32().unwrap(),
            workerdata.id
        )
        .execute(&pool)
        .await?;

        Ok(Redirect::to(
            format!("/admin/worker-edit?worker={}", workerdata.id).as_str(),
        ))

}
