use std::convert::Infallible;
use std::env;

use crate::Backend;
use crate::{errors::CustomError, AppEngine, AppState, Job, JobWorker};
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect},
    Form, Json,
};
use axum_login::AuthSession;
use axum_template::RenderHtml;
use git_version::git_version;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{query, query_as, query_scalar, Pool, Sqlite};
use time::Date;
use utoipa::ToSchema;

/// The summary statistics shown on the homepage: total jobs/miles/workers
/// tracked, and a few rough daily/monthly averages. This is public
/// information (shown to logged-out visitors too), so unlike most of the
/// other `/api/v1/*` endpoints, this one requires no authentication.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct HomeStats {
    pub jobs: i64,
    pub jobs_avg: String,
    pub miles: String,
    pub miles_avg: String,
    pub workers: i64,
    pub workers_avg: String,
    pub site_url: String,
}

/// Shared business logic for both the homepage and its JSON API
/// equivalent. This never actually fails - every query has a sensible
/// fallback - so unlike most `*_core` functions elsewhere, this one isn't
/// fallible.
async fn index_core(pool: &Pool<Sqlite>) -> HomeStats {
    let jobs = query_scalar!(
        r#"
        select count(*) from jobs;   
    "#
    )
    .fetch_one(pool)
    .await;
    let jobs = match jobs {
        Ok(v) => v,
        _ => 0,
    };

    let workers = query_scalar!(
        r#"
        select count(*) from users where admin = false and deactivated = false;   
    "#
    )
    .fetch_one(pool)
    .await;
    let workers = match workers {
        Ok(v) => v,
        _ => 0,
    };

    let miles = query_scalar!(
        r#"
        select sum(miles_driven) from jobworkers;   
    "#
    )
    .fetch_one(pool)
    .await;
    let miles = match miles {
        Ok(Some(v)) => v,
        _ => 0.0,
    };

    let earliest: Result<time::Date, sqlx::Error> = query_scalar!(
        r#"
        select date from jobs order by date asc;   
    "#
    )
    .fetch_one(pool)
    .await;
    let days = match earliest {
        Ok(v) => (time::OffsetDateTime::now_utc().date() - v).whole_days(),
        _ => 0,
    };

    let mut jobsavg = days as f64 / jobs as f64;
    if !jobsavg.is_finite() {
        jobsavg = 0.0
    };

    let mut milesavg = miles as f64 / days as f64;
    if !milesavg.is_finite() {
        milesavg = 0.0
    };

    let mut workersavg = (days as f64 / 30.437) / workers as f64;
    if !workersavg.is_finite() {
        workersavg = 0.0
    };

    HomeStats {
        jobs,
        jobs_avg: format!("{:.2}", jobsavg),
        miles: format!("{:.2}", miles),
        miles_avg: format!("{:.2}", milesavg),
        workers,
        workers_avg: format!("{:.2}", workersavg),
        site_url: env::var("SITE_URL").expect("SITE_URL not set"),
    }
}

/// `GET /` — HTML-facing endpoint.
pub(crate) async fn index(
    State(AppState { pool, engine, .. }): State<AppState>,

    auth: AuthSession<Backend>,
) -> Result<impl IntoResponse, Infallible> {
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.user.is_some();

    let stats = index_core(&pool).await;

    let data = serde_json::json!({
    "git_ver": git_version!(),
        "admin": admin,
        "logged_in": logged_in,
        "title": "CZ4R",
        "jobs": stats.jobs,
        "jobsavg": stats.jobs_avg,
        "miles": stats.miles,
        "milesavg": stats.miles_avg,
        "workers": stats.workers,
        "workersavg": stats.workers_avg,
        "siteurl": stats.site_url
    });

    Ok(RenderHtml("home.hbs", engine, data))
}

/// `GET /api/v1/stats` — REST/JSON endpoint. Public: the homepage shows
/// these same numbers to logged-out visitors, so this doesn't require a
/// bearer token either.
#[utoipa::path(
    get,
    path = "/api/v1/stats",
    responses((status = OK, body = HomeStats)),
    tag = super::USER_TAG
)]
pub(crate) async fn index_api(
    State(AppState { pool, .. }): State<AppState>,
) -> Json<HomeStats> {
    Json(index_core(&pool).await)
}
