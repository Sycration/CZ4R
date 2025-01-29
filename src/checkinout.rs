use crate::{errors::CustomError, AppState, Job, JobWorker};
use crate::{get_user, Backend};
use anyhow::anyhow;
use axum::http::StatusCode;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect},
    Form,
};
use axum_login::AuthSession;
use axum_template::RenderHtml;
use git_version::git_version;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use sqlx::{query, query_as, Pool};
use time::format_description::well_known::Iso8601;
use time::{format_description, macros::format_description, Time};
use tracing::*;

#[derive(Deserialize)]
pub(crate) struct CheckInOutPage {
    id: i64,
    worker: i64,
}

pub(crate) async fn checkinoutpage(
    State(AppState { pool, engine, .. }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<CheckInOutPage>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    let (my_id, my_name, admin) = get_user(&auth)?;

    let worker = form.worker;

    if !admin && worker != my_id {
        debug!(
            "user {} (id {}) tried to check in for user {}",
            my_name, my_id, worker
        );
        return Err(CustomError(anyhow!(
            "Attempted to check in for other worker"
        )));
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

pub(crate) async fn checkinout(
    State(AppState { pool, .. }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<CheckInOutForm>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    let (my_id, my_name, admin) = get_user(&auth)?;

    let worker = form.WorkerId;

    if !admin && worker != my_id {
        return Err(CustomError(anyhow!(
            "Attempted to check in for other worker"
        )));
    }

    let signin = form.Signin.unwrap_or_default();
    let signout = form.Signout.unwrap_or_default();
    let milesdriven = form.MilesDriven.unwrap_or_default();
    let hoursdriven = form.HoursDriven.unwrap_or_default();
    let minutesdriven = form.MinutesDriven.unwrap_or_default();
    let extraexpenses = form.ExtraExpenses.unwrap_or_default();

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
        form.Notes,
        worker,
        form.JobId
    )
    .execute(&pool)
    .await?;

    info!(
        "job {} assigned to user {} updated by {} {} (id {}):\n
sign in time: {}\n
sign out time: {}\n
miles driven: {}\n
hours driven: {}\n
extra expenses (cents): {}\n
notes: {}",
        form.JobId,
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
        form.Notes.unwrap_or_default(),
    );

    Ok(StatusCode::OK.into_response())
}
