use std::env;

use crate::Backend;
use crate::{errors::CustomError, AppEngine, AppState, Job, JobWorker};
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect},
    Form,
};
use axum_login::AuthSession;
use axum_template::RenderHtml;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use sqlx::{query, query_as, query_scalar, Pool, Postgres};
use time::Date;

pub(crate) async fn index(
    State(AppState { pool, engine }): State<AppState>,

    mut auth: AuthSession<Backend>,
) -> Result<impl IntoResponse, CustomError> {
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.user.is_some();

    let jobs = query_scalar!(
        r#"
        select count(*) from jobs;   
    "#
    )
    .fetch_one(&pool)
    .await;
    let jobs = match jobs {
        Ok(Some(v)) => v,
        Ok(None) => return Err(CustomError::Database("Invalid number of jobs".to_string())),
        Err(e) => return Err(CustomError::Database(e.to_string())),
    };



    let workers = query_scalar!(
        r#"
        select count(*) from users where admin = false and deactivated = false;   
    "#
    )
    .fetch_one(&pool)
    .await;
    let workers = match workers {
        Ok(Some(v)) => v,
        Ok(None) => {
            return Err(CustomError::Database(
                "Invalid number of workers".to_string(),
            ))
        }
        Err(e) => return Err(CustomError::Database(e.to_string())),
    };

    let miles = query_scalar!(
        r#"
        select sum(miles_driven) from jobworkers;   
    "#
    )
    .fetch_one(&pool)
    .await;
    let miles = match miles {
        Ok(Some(v)) => v,
        Ok(None) => {
            return Err(CustomError::Database(
                "Invalid number of total miles driven".to_string(),
            ))
        }
        Err(e) => return Err(CustomError::Database(e.to_string())),
    };

    let earliest: Result<time::Date, sqlx::Error> = query_scalar!(
        r#"
        select date from jobs order by date asc;   
    "#
    )
    .fetch_one(&pool)
    .await;
    let earliest = match earliest {
        Ok(v) => v,
        Err(e) => return Err(CustomError::Database(e.to_string())),
    };
    let days = time::OffsetDateTime::now_utc().date() - earliest;

    let jobsavg = days.whole_days() as f64 / jobs as f64;
    let milesavg =  miles as f64/ days.whole_days() as f64;
    let workersavg = (days.whole_days() as f64 / 30.437) / workers as f64;

    let data = serde_json::json!({
        "admin": admin,
        "logged_in": logged_in,
        "title": "CZ4R",
        "jobs": jobs,
        "jobsavg": format!("{:.2}", jobsavg),
        "miles": format!("{:.2}", miles),
        "milesavg": format!("{:.2}", milesavg),
        "workers": workers,
        "workersavg": format!("{:.2}", workersavg),
        "siteurl": env::var("SITE_URL").expect("SITE_URL not set")
    });

    Ok(RenderHtml("home.hbs", engine, data))
}
