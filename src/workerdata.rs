use axum::{
    extract::State,
    response::{Html, IntoResponse, Redirect},
    Form,
};

use axum_template::RenderHtml;
use password_hash::{rand_core::le, PasswordHasher, Salt, SaltString};
use serde::{Deserialize, Serialize};
use sqlx::types::time::Date;
use sqlx::{query, query_as, Pool, Postgres};
use time::{OffsetDateTime, Time};

use crate::{get_admin, Backend};
use crate::{
    errors::{self, CustomError},
    now, AppState, Worker,
};
use axum_login::AuthSession;
#[derive(Deserialize)]
pub(crate) struct WorkerDataForm {
    worker: Option<i64>,
    start_date: Option<Date>,
    end_date: Option<Date>,
}

#[derive(Serialize, Default, Debug)]
pub struct WDEntry {
    pub JobId: i64,
    pub WorkerId: i64,
    pub Date: String,
    pub Location: String,
    pub FlatRate: bool,
    pub HoursWorked: String,
    pub TrueHoursWorked: String,
    pub HoursDriven: String,
    pub MilesDriven: String,
    pub ExtraExpCents: String,
}

fn hours_worked(signin: Time, signout: Time) -> f32 {
    ((signout - signin).as_seconds_f32() / 3600.).max(1.0)
}

pub(crate) async fn workerdatapage(
    State(AppState { pool, engine }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(worker): Form<WorkerDataForm>,
) -> Result<impl IntoResponse, CustomError> {
    get_admin(auth)?;

    let users = sqlx::query_as!(
        Worker,
        "
    select * from users
        where deactivated = false;
    "
    )
    .fetch_all(&pool)
    .await?;

    let selectlist = users
        .iter()
        .map(|w| (w.id, w.name.as_str()))
        .collect::<Vec<_>>();

    let date = now().date();

    let mut from = String::new();
    let mut to = String::new();

    let (entries, totals) = if let Some(id) = worker.worker {
        let start_date = if let Some(d) = worker.start_date {
            d
        } else if date.day() <= 15 {
            date.replace_day(1).unwrap()
        } else {
            date.replace_day(16).unwrap()
        };

        let end_date = if let Some(d) = worker.end_date {
            d
        } else {
            date
        };

        from = start_date.to_string();
        to = end_date.to_string();

        let data = sqlx::query!(
            r#"
        select jobworkers.*, jobs.date, jobs.sitename from jobworkers
            inner join jobs
            on jobs.id = jobworkers.job
            
        where
            jobworkers.worker = $1
        and
            jobs.date >= $2 and jobs.date <= $3
        order by date desc;
    "#,
            id,
            start_date,
            end_date
        )
        .fetch_all(&pool)
        .await?;

        let hours_worked_total = data
            .iter()
            .filter(|d| d.signin.is_some() && d.signout.is_some())
            .fold(0.0, |acc, x| {
                acc + hours_worked(x.signin.unwrap(), x.signout.unwrap())
            });
        let hours_driven_total = data
            .iter()
            .filter(|d| d.signin.is_some() && d.signout.is_some())
            .fold(0.0, |acc, x| acc + x.hours_driven);
        let miles_driven_total = data
            .iter()
            .filter(|d| d.signin.is_some() && d.signout.is_some())
            .fold(0.0, |acc, x| acc + x.miles_driven);
        let extra_exp_total = data
            .iter()
            .filter(|d| d.signin.is_some() && d.signout.is_some())
            .fold(0, |acc, x| acc + x.extraexpcents);

        let entries = data
            .into_iter()
            .filter(|d| d.signin.is_some() && d.signout.is_some())
            .map(|d| WDEntry {
                Date: d.date.to_string(),
                Location: d.sitename,
                FlatRate: d.using_flat_rate,
                HoursWorked: {
                    let val = hours_worked(d.signin.unwrap(), d.signout.unwrap());
                    format!("{:.2}", val)
                },
                TrueHoursWorked: {
                    let val = (d.signout.unwrap() - d.signin.unwrap()).as_seconds_f32() / 3600.;
                    format!("{:.2}", val)
                },
                HoursDriven: format!("{:.2}", d.hours_driven),
                MilesDriven: format!("{:.2}", d.miles_driven),
                ExtraExpCents: format!("{:.2}", (d.extraexpcents as f64 / 100.)),
                WorkerId: d.worker,
                JobId: d.job,
            })
            .collect::<Vec<_>>();

        let totals = WDEntry {
            Date: String::new(),
            Location: String::new(),
            FlatRate: false,
            HoursWorked: format!("{:.2}", hours_worked_total),
            TrueHoursWorked: String::new(),
            HoursDriven: format!("{:.2}", hours_driven_total),
            MilesDriven: format!("{:.2}", miles_driven_total),
            ExtraExpCents: format!("{:.2}", (extra_exp_total as f64 / 100.)),
            JobId: -1,
            WorkerId: -1,
        };

        (entries, totals)
    } else {
        (vec![], WDEntry::default())
    };

    let data = serde_json::json!({
        "title": "CZ4R Worker Data",
        "admin": true,
        "logged_in": true,
        "selected": worker.worker,
        "workerlist": users,
        "selectlist": selectlist,
        "num_jobs": entries.len(),
        "entries": entries,
        "totals": totals,
        "from": from,
        "to": to,
        "target": "worker-data"
    });

    Ok(RenderHtml("workerdata.hbs", engine, data))
}
