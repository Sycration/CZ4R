use crate::api_auth::ApiAuth;
use crate::errors::{to_api, ApiError, CustomError};
use crate::{current_user, get_user, Backend};
use crate::{AppState, Job, JobWorker};
use anyhow::anyhow;
use axum::http::StatusCode;
use axum::{
    extract::State,
    response::{IntoResponse, Redirect},
    Form, Json,
};
use axum_login::AuthSession;
use axum_template::RenderHtml;
use git_version::git_version;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{query, query_as, Pool};
use time::format_description::well_known::Iso8601;
use time::{format_description, macros::format_description, Time};
use tracing::*;
use utoipa::ToSchema;

#[derive(Deserialize)]
pub(crate) struct CheckInOutPage {
    id: i64,
    worker: i64,
}

pub(crate) async fn checkinoutpage(
    State(AppState { pool, engine, .. }): State<AppState>,
    auth: AuthSession<Backend>,
    Form(form): Form<CheckInOutPage>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    let (my_id, my_name, admin) = get_user(current_user(&auth).as_ref())?;

    let worker = form.worker;

    if !admin && worker != my_id {
        debug!(
            "user {} (id {}) tried to check in for user {}",
            my_name, my_id, worker
        );
        return Err(CustomError::new(
            anyhow!("Attempted to check in for other worker"),
            StatusCode::FORBIDDEN,
        ));
    }

    let jw = query!(
        r#"
        select * from jobworkers
            where 
            job = $1 
                and
            worker = $2;            
    "#,
        form.id,
        worker
    )
    .fetch_one(&pool)
    .await?;

    let job = query_as!(
        Job,
        r#"
        select * from jobs
            where 
            id = $1;           
    "#,
        form.id
    )
    .fetch_one(&pool)
    .await?;

    let signin = jw.signin.map(|t| {
        Time::parse(&t, &Iso8601::TIME)
            .unwrap()
            .format(&format_description::parse("[hour]:[minute]").unwrap())
            .unwrap()
    });
    let signout = jw.signout.map(|t| {
        Time::parse(&t, &Iso8601::TIME)
            .unwrap()
            .format(&format_description::parse("[hour]:[minute]").unwrap())
            .unwrap()
    });

    let data = json!({
    "git_ver": git_version!(),
        "title": "CZ4R Time Tracking",
        "admin": admin,
        "logged_in": true,
        "job_id": form.id,
        "worker_id": form.worker,
        "work_order": job.workorder.as_str(),
        "service_code": job.servicecode.as_str(),
        "site_name": job.sitename.as_str(),
        "address": job.address.as_str(),
        "date": format!("{} {}, {}", job.date.month(), job.date.day(),  job.date.year()),
        "signin": signin.unwrap_or_default(),
        "signout": signout.unwrap_or_default(),
        "miles": jw.miles_driven,
        "hours": jw.hours_driven.floor(),
        "minutes": 60. * (jw.hours_driven - jw.hours_driven.floor()),
        "extra_exp_ct": format!("{:.2}", (jw.extraexpcents as f64 / 100.)),
        "notes": jw.notes.as_str(),
        "jobnotes": job.notes.as_str(),
    });

    Ok(RenderHtml("checkinout.hbs", engine, data))
}

/// The data needed to update a single worker's check-in/out record for a
/// job. Used as the request body for both the web form and the JSON API.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub(crate) struct CheckInOutInput {
    pub signin: Option<String>,
    pub signout: Option<String>,
    pub miles_driven: Option<f32>,
    pub hours_driven: Option<f32>,
    pub minutes_driven: Option<f32>,
    pub extra_expenses: Option<String>,
    pub notes: Option<String>,
    pub job_id: i64,
    pub worker_id: i64,
}

/// The legacy, PascalCase-named form fields the HTML/htmx client posts.
//?Signin=&Signout=&MilesDriven=2&ExtraExpenses=&Notes=
#[derive(Deserialize)]
pub(crate) struct CheckInOutForm {
    Signin: Option<String>,
    Signout: Option<String>,
    MilesDriven: Option<f32>,
    HoursDriven: Option<f32>,
    MinutesDriven: Option<f32>,
    ExtraExpenses: Option<String>,
    Notes: Option<String>,
    JobId: i64,
    WorkerId: i64,
}

impl From<CheckInOutForm> for CheckInOutInput {
    fn from(form: CheckInOutForm) -> Self {
        Self {
            signin: form.Signin,
            signout: form.Signout,
            miles_driven: form.MilesDriven,
            hours_driven: form.HoursDriven,
            minutes_driven: form.MinutesDriven,
            extra_expenses: form.ExtraExpenses,
            notes: form.Notes,
            job_id: form.JobId,
            worker_id: form.WorkerId,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct CheckInOutOutput {
    pub job_id: i64,
    pub worker_id: i64,
}

/// Shared business logic for updating a check-in/out record. Both the web
/// handler and the API handler call this and only differ in how they
/// extract their input and render their output.
async fn checkinout_core(
    pool: &Pool<sqlx::Sqlite>,
    user: Option<&crate::CurrentUser>,
    input: CheckInOutInput,
) -> Result<CheckInOutOutput, CustomError> {
    let (my_id, my_name, admin) = get_user(user)?;

    let worker = input.worker_id;

    if !admin && worker != my_id {
        return Err(CustomError::new(
            anyhow!("Attempted to check in for other worker"),
            StatusCode::FORBIDDEN,
        ));
    }

    let signin = input.signin.unwrap_or_default();
    let signout = input.signout.unwrap_or_default();
    let milesdriven = input.miles_driven.unwrap_or_default();
    let hoursdriven = input.hours_driven.unwrap_or_default();
    let minutesdriven = input.minutes_driven.unwrap_or_default();
    let extraexpenses = input.extra_expenses.unwrap_or_default();

    let extraexp = Decimal::from_str_exact(&extraexpenses)? * Decimal::ONE_HUNDRED;

    let signin = if signin.is_empty() {
        None
    } else {
        Some(Time::parse(
            &signin,
            format_description!("[hour]:[minute]"),
        )?)
    };

    let signout = if signout.is_empty() {
        None
    } else {
        Some(Time::parse(
            &signout,
            format_description!("[hour]:[minute]"),
        )?)
    };

    let true_hours_driven = hoursdriven + (minutesdriven / 60.);
    let true_extra_exp = extraexp.to_i32().unwrap();

    query!(
        r#"
    update jobworkers
    set
        signin = $1,
        signout = $2,
        miles_driven = $3,
        hours_driven = $4,
        extraexpcents = $5,
        notes = $6
    where worker = $7
    and job = $8;
    "#,
        signin,
        signout,
        milesdriven,
        true_hours_driven,
        true_extra_exp,
        input.notes,
        worker,
        input.job_id
    )
    .execute(pool)
    .await?;

    info!(
        "job {} assigned to user {} updated by {} {} (id {}):\n
sign in time: {}\n
sign out time: {}\n
miles driven: {}\n
hours driven: {}\n
extra expenses (cents): {}\n
notes: {}",
        input.job_id,
        worker,
        if admin { "admin" } else { "user" },
        my_name,
        my_id,
        signin
            .map(|t| t.to_string())
            .unwrap_or("removed".to_string()),
        signout
            .map(|t| t.to_string())
            .unwrap_or("removed".to_string()),
        milesdriven,
        true_hours_driven,
        true_extra_exp,
        input.notes.unwrap_or_default(),
    );

    Ok(CheckInOutOutput {
        job_id: input.job_id,
        worker_id: worker,
    })
}

/// `POST /web/v1/checkinout` — HTML/htmx-facing endpoint. Takes a
/// form-encoded body, returns a bare status code (the client re-fetches the
/// page via htmx on success).
pub(crate) async fn checkinout(
    State(AppState { pool, .. }): State<AppState>,
    auth: AuthSession<Backend>,
    Form(form): Form<CheckInOutForm>,
) -> Result<impl IntoResponse, CustomError> {
    checkinout_core(&pool, current_user(&auth).as_ref(), form.into()).await?;
    Ok(StatusCode::OK.into_response())
}

/// `POST /api/v1/checkinout` — REST/JSON endpoint. Takes and returns JSON.
#[utoipa::path(
    post,
    path = "/api/v1/checkinout",
    request_body = CheckInOutInput,
    responses((status = OK, body = CheckInOutOutput)),
    security(("bearer_auth" = [])),
    tag = super::USER_TAG
)]
pub(crate) async fn checkinout_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
    Json(input): Json<CheckInOutInput>,
) -> Result<Json<CheckInOutOutput>, ApiError> {
    to_api(checkinout_core(&pool, Some(&user), input).await)
}
