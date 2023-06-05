use axum::{extract::{State, Path}, response::{Html, Redirect}, Form};
use rust_decimal::Decimal;
use serde::Deserialize;
use sqlx::{Pool, Postgres, query, query_as};
use time::{Time, macros::format_description, format_description};
use rust_decimal::prelude::*;
use crate::{Auth, errors::CustomError, JobWorker, Job};

#[derive(Deserialize)]
pub(crate) struct CheckInOutPage {
    id: i64,
}

pub(crate) async fn checkinoutpage(
    State(pool): State<Pool<Postgres>>,
    mut auth: Auth,
    Form(form): Form<CheckInOutPage>
) -> Result<Html<String>, CustomError> {

    let admin = auth.current_user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.current_user.is_some();

    if !logged_in {
        return Err(CustomError::Auth("Not logged in".to_string()));
    }

    let jw = query_as!(JobWorker, r#"
        select * from jobworkers
            where 
            job = $1 
                and
            worker = $2;            
    "#, form.id, auth.current_user.unwrap().id).fetch_one(&pool).await;
    let jw = match jw {
        Ok(v) => v,
        Err(e) => return Err(CustomError::Database(e.to_string())),
    };

    let job = query_as!(Job, r#"
        select * from jobs
            where 
            id = $1;           
    "#, form.id).fetch_one(&pool).await;
    let job = match job {
        Ok(v) => v,
        Err(e) => return Err(CustomError::Database(e.to_string())),
    };

    let signin = jw.signin.map(|t|t.format(&format_description::parse("[hour]:[minute]").unwrap()).unwrap());
    let signout = jw.signout.map(|t|t.format(&format_description::parse("[hour]:[minute]").unwrap()).unwrap());

    Ok(crate::render(|buf| {
        crate::templates::checkinout_html(
            buf,
            "CZ4R Time Tracking",
            admin,
            form.id,
            job.sitename.as_str(),
            job.address.as_str(),
            job.date.to_string().as_str(),
            signin.unwrap_or_default().as_str(),
            signout.unwrap_or_default().as_str(),
            jw.miles_driven,
            jw.extraexpcents,
            jw.notes.as_str()
        )
    }))
}


//?Signin=&Signout=&MilesDriven=2&ExtraExpenses=&Notes=
#[derive(Deserialize)]
pub(crate) struct CheckInOutForm {
    Signin: String,
    Signout: String,
    MilesDriven: i32,
    ExtraExpenses: String,
    Notes: String,
    JobId: i64,
}

pub(crate) async fn checkinout(
    State(pool): State<Pool<Postgres>>,
    mut auth: Auth,
    Form(form): Form<CheckInOutForm>,
) -> Result<Redirect, CustomError> {
    if auth.current_user.is_none() {
        return Err(CustomError::Auth("Not logged in".to_string()));
    }

    let extraexp = Decimal::from_str_exact(&form.ExtraExpenses);
    let extraexp = if let Ok(v) = extraexp {
        v * Decimal::ONE_HUNDRED
    } else {
        return Err(CustomError::ClientData(format!(
            "{} is not a number",
            form.ExtraExpenses
        )));
    };

    let signin = if form.Signin.is_empty() {None} else {
        match Time::parse(&form.Signin, format_description!("[hour]:[minute]")) {
            Ok(t) => Some(t),
            Err(_) => {return Err(CustomError::ClientData(format!("{} is not a valid time in the format [hour]:[minute]", form.Signin)));},
        }
    };

    let signout = if form.Signout.is_empty() {None} else {
        match Time::parse(&form.Signout, format_description!("[hour]:[minute]")) {
            Ok(t) => Some(t),
            Err(_) => {return Err(CustomError::ClientData(format!("{} is not a valid time in the format [hour]:[minute]", form.Signin)));},
        }
    };



    let res = query!(r#"
    update jobworkers
    set
        signin = $1,
        signout = $2,
        miles_driven = $3,
        extraexpcents = $4,
        notes = $5;
    "#, signin, signout, form.MilesDriven, extraexp.to_i32().unwrap(), form.Notes).execute(&pool).await;
    if let Err(e) = res {return Err(CustomError::Database(e.to_string()));}

    Ok(Redirect::to(format!("/checkinout?id={}", form.JobId).as_str()))

}