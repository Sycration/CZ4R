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
    State(AppState { pool, engine }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(workerdata): Form<WorkerChangeForm>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    if let Some(true) = auth.user.map(|u| u.admin) {
        let hourly = Decimal::from_str_exact(&workerdata.Hourly);
        let hourly = if let Ok(v) = hourly {
            v * Decimal::ONE_HUNDRED
        } else {
            return Err(CustomError::ClientData(format!(
                "{} is not a number",
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
        let res = query!(
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
        .await;

        if let Err(e) = res {
            return Err(CustomError::Database(e.to_string()).build(&engine));
        }
        Ok(Redirect::to(
            format!("/admin/worker-edit?worker={}", workerdata.id).as_str(),
        ))
    } else {
        Err(CustomError::Auth("Not logged in as admin".to_string()).build(&engine))
    }
}
