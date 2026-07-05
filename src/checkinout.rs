use crate::api_auth::ApiAuth;
use crate::errors::{ApiError, CustomError, to_api};
use crate::jobedit::JobDetail;
use crate::{AppState, Job, JobWorker};
use crate::{Backend, current_user, get_user};
use anyhow::anyhow;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::{
    Form, Json,
    extract::State,
    response::{IntoResponse, Redirect},
};
use axum_login::AuthSession;
use axum_template::RenderHtml;
use git_version::git_version;
use rust_decimal::Decimal;
use rust_decimal::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Pool, Sqlite, query, query_as};
use time::Date;
use time::format_description::well_known::Iso8601;
use time::{Time, format_description, macros::format_description};
use tracing::*;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub(crate) struct CheckInOutPage {
    id: i64,
    worker: i64,
}

#[derive(Debug, Clone, ToSchema, Serialize)]

pub struct FullAssignmentData {
    sitename: String,
    workorder: String,
    servicecode: String,
    address: String,
    date: Date,
    job_notes: String,
    worker_notes: String,
    job_id: i64,
    worker_id: i64,
    signin: Option<String>,
    signout: Option<String>,
    miles_driven: f32,
    hours_driven: f32,
    minutes_driven: f32,
    extraexpcents: i64,
    using_flat_rate: bool,
}


pub(crate) async fn assignment_data_core(
    // State(AppState { pool, engine, .. }): State<AppState>,
    // auth: AuthSession<Backend>,
    pool: &Pool<Sqlite>,
    user: Option<&crate::CurrentUser>,
    form: &CheckInOutPage,
) -> Result<FullAssignmentData, CustomError> {
    let (my_id, my_name, admin) = get_user(user)?;

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
    .fetch_one(pool)
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
    .fetch_one(pool)
    .await?;

    Ok(FullAssignmentData {
        sitename: job.sitename,
        workorder: job.workorder,
        servicecode: job.servicecode,
        address: job.address,
        date: job.date,
        job_notes: job.notes,
        worker_notes: jw.notes,
        job_id: jw.job,
        worker_id: jw.worker,
        signin: jw.signin,
        signout: jw.signout,
        miles_driven: jw.miles_driven as f32,
        hours_driven: (jw.hours_driven as f32).floor(),
        minutes_driven: ((jw.hours_driven as f32).fract() * 60.).round(),
        extraexpcents: jw.extraexpcents,
        using_flat_rate: jw.using_flat_rate,
    })
}

/// `GET /checkinout` — HTML-facing endpoint.
pub(crate) async fn checkinoutpage(
    State(AppState { pool, engine, .. }): State<AppState>,
    auth: AuthSession<Backend>,
    Form(form): Form<CheckInOutPage>,
) -> Result<impl IntoResponse, CustomError> {
    let user = current_user(&auth);
    let user_data = get_user(user.as_ref())?;
    let full_data = assignment_data_core(&pool, user.as_ref(), &form).await?;


    let signin = full_data.signin.map(|s| Time::parse(&s, &Iso8601::DEFAULT).map(|t| t.format(&format_description!("[hour]:[minute]"))).ok().transpose().ok()).flatten().flatten();
    let signout = full_data.signout.map(|s| Time::parse(&s, &Iso8601::DEFAULT).map(|t| t.format(&format_description!("[hour]:[minute]"))).ok().transpose().ok()).flatten().flatten();

    let data = json!({
    "git_ver": git_version!(),
        "title": "CZ4R Time Tracking",
        "admin": user_data.2,
        "logged_in": true,
        "job_id": full_data.job_id,
        "worker_id": full_data.worker_id,
        "work_order": full_data.workorder.as_str(),
        "service_code": full_data.servicecode.as_str(),
        "site_name": full_data.sitename.as_str(),
        "address": full_data.address.as_str(),
        "date": format!("{} {}, {}", full_data.date.month(), full_data.date.day(),  full_data.date.year()),
        "signin": signin,
        "signout": signout,
        "miles": full_data.miles_driven,
        "hours": full_data.hours_driven.floor(),
        "minutes": full_data.minutes_driven,
        "extra_exp_ct": format!("{:.2}", (full_data.extraexpcents as f64 / 100.)),
        "notes": full_data.worker_notes.as_str(),
        "jobnotes": full_data.job_notes.as_str(),
    });

    Ok(RenderHtml("checkinout.hbs", engine, data))
}

#[utoipa::path(
    get,
    path = "/api/v1/assignment-data",
    params(("id" = i64, Query, description = "Job ID"), ("worker" = i64, Query, description = "Worker ID")),
    responses((status = OK, body = FullAssignmentData)),
    security(("bearer_auth" = [])),
    tag = super::USER_TAG
)]
pub(crate) async fn assignment_data_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
    Query(input): Query<CheckInOutPage>,
) -> Result<Json<FullAssignmentData>, ApiError> {
    to_api(assignment_data_core(&pool, Some(&user), &input).await)
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
        ).or(Time::parse(
            &signin,
            &Iso8601::DEFAULT,
        ))?)
    };

    let signout = if signout.is_empty() {
        None
    } else {
        Some(Time::parse(
            &signout,
            format_description!("[hour]:[minute]"),
        ).or(Time::parse(
            &signout,
            &Iso8601::DEFAULT,
        ))?)
    };

    let true_hours_driven = hoursdriven + (minutesdriven / 60.);
    let true_extra_exp = extraexp.to_i32().unwrap();

    let worker_notes = input.notes.unwrap_or_default();
    let query_notes = worker_notes.clone();
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
        query_notes,
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
        &worker_notes,
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

/// Sign in and sign out parameters accept full ISO8601 or simple HH:MM time strings.
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
