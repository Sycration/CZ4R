use crate::Backend;
use crate::{errors::CustomError, AppState, Job, JobWorker};
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
use sqlx::{query, query_as, Pool, Postgres};
use time::{format_description, macros::format_description, Time};

#[derive(Deserialize)]
pub(crate) struct CheckInOutPage {
    id: i64,
    worker: i64,
}

pub(crate) async fn checkinoutpage(
    State(AppState { pool, engine }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<CheckInOutPage>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.user.is_some();

    if !logged_in {
        return Err(CustomError::Auth("Not logged in".to_string()).build(&engine));
    }

    let worker = form.worker;

    if !admin && worker != auth.user.as_ref().unwrap().id {
        return Err(CustomError::Auth(
            "Attempted to check in for other worker".to_string(),
        ).build(&engine));
    }

    let jw = query_as!(
        JobWorker,
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
    .await;
    let jw = match jw {
        Ok(v) => v,
        Err(e) => return Err(CustomError::Database(e.to_string()).build(&engine)),
    };

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
    .await;
    let job = match job {
        Ok(v) => v,
        Err(e) => return Err(CustomError::Database(e.to_string()).build(&engine)),
    };

    let signin = jw.signin.map(|t| {
        t.format(&format_description::parse("[hour]:[minute]").unwrap())
            .unwrap()
    });
    let signout = jw.signout.map(|t| {
        t.format(&format_description::parse("[hour]:[minute]").unwrap())
            .unwrap()
    });

    let data = json!({
        "title": "CZ4R Time Tracking",
        "admin": admin,
        "logged_in": logged_in,
        "job_id": form.id,
        "worker_id": form.worker,
        "work_order": job.workorder.as_str(),
        "service_code": job.servicecode.as_str(),
        "site_name": job.sitename.as_str(),
        "address": job.address.as_str(),
        "date": format!("{} {}, {}", job.date.month(), job.date.day(),  job.date.year()),
        "signin": signin.unwrap_or_default().as_str(),
        "signout": signout.unwrap_or_default().as_str(),
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
State(AppState { pool, engine }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<CheckInOutForm>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    if auth.user.is_none() {
        return Err(CustomError::Auth("Not logged in".to_string()).build(&engine));
    }
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);

    let worker = form.WorkerId;

    if !admin && worker != auth.user.as_ref().unwrap().id {
        return Err(CustomError::Auth(
            "Attempted to check in for other worker".to_string(),
        ).build(&engine));
    }

    let signin = form.Signin.unwrap_or_default();
    let signout = form.Signout.unwrap_or_default();
    let milesdriven = form.MilesDriven.unwrap_or_default();
    let hoursdriven = form.HoursDriven.unwrap_or_default();
    let minutesdriven = form.MinutesDriven.unwrap_or_default();
    let extraexpenses = form.ExtraExpenses.unwrap_or_default();

    let extraexp = Decimal::from_str_exact(&extraexpenses);
    let extraexp = if let Ok(v) = extraexp {
        v * Decimal::ONE_HUNDRED
    } else {
        return Err(CustomError::ClientData(format!(
            "{} is not a number",
            extraexpenses
        )).build(&engine));
    };

    let signin = if signin.is_empty() {
        None
    } else {
        match Time::parse(&signin, format_description!("[hour]:[minute]")) {
            Ok(t) => Some(t),
            Err(_) => {
                return Err(CustomError::ClientData(format!(
                    "{} is not a valid time in the format [hour]:[minute]",
                    signin
                )).build(&engine));
            }
        }
    };

    let signout = if signout.is_empty() {
        None
    } else {
        match Time::parse(&signout, format_description!("[hour]:[minute]")) {
            Ok(t) => Some(t),
            Err(_) => {
                return Err(CustomError::ClientData(format!(
                    "{} is not a valid time in the format [hour]:[minute]",
                    signout
                )).build(&engine));
            }
        }
    };

    let res = query!(
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
        (hoursdriven + (minutesdriven / 60.)),
        extraexp.to_i32().unwrap(),
        form.Notes,
        worker,
        form.JobId
    )
    .execute(&pool)
    .await;
    if let Err(e) = res {
        return Err(CustomError::Database(e.to_string()).build(&engine));
    }

    Ok(Redirect::to(
        format!("/checkinout?id={}&worker={}", form.JobId, worker).as_str(),
    ))
}
