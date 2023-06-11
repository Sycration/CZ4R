use axum::{
    extract::State,
    response::{Html, Redirect},
    Form,
};

use password_hash::{rand_core::le, PasswordHasher, Salt, SaltString};
use serde::Deserialize;
use sqlx::types::time::Date;
use sqlx::{query, query_as, Pool, Postgres};
use time::{OffsetDateTime, Time};

use crate::{
    errors::{self, CustomError},
    now, Auth, Worker,
};

#[derive(Deserialize)]
pub(crate) struct WorkerDataForm {
    worker: Option<i64>,
    start_date: Option<Date>,
    end_date: Option<Date>,
}

pub struct WDEntry {
    pub Date: String,
    pub Location: String,
    pub FlatRate: bool,
    pub HoursWorked: f32,
    pub HoursDriven: f32,
    pub ExtraExpCents: i32,
}

pub(crate) async fn workerdatapage(
    State(pool): State<Pool<Postgres>>,
    mut auth: Auth,
    Form(worker): Form<WorkerDataForm>,
) -> Result<Html<String>, CustomError> {
    let admin = auth.current_user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.current_user.is_some();

    if !admin {
        return Err(CustomError::Auth("Not logged in as admin".to_string()));
    }

    let users = sqlx::query_as!(
        Worker,
        "
    select * from users
    "
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let selectlist = users
        .iter()
        .map(|w| (w.id, w.name.as_str()))
        .collect::<Vec<_>>();

    let date = now().date();

    let mut from = String::new();
    let mut to = String::new();

    let entries = if let Some(id) = worker.worker {
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
        .await;
        let data = match data {
            Ok(d) => d,
            Err(e) => {
                return Err(CustomError::Database(e.to_string()));
            }
        };

        Some(
            data.into_iter()
                .filter(|d| d.signin.is_some() && d.signout.is_some())
                .map(|d| WDEntry {
                    Date: d.date.to_string(),
                    Location: d.sitename,
                    FlatRate: d.using_flat_rate,
                    HoursWorked: (d.signout.unwrap() - d.signin.unwrap()).as_seconds_f32() / 3600.,
                    HoursDriven: d.hours_driven,
                    ExtraExpCents: d.extraexpcents,
                })
                .collect::<Vec<_>>(),
        )
    } else {
        None
    };

    let hours_worked_total = entries
        .as_ref()
        .map(|e| e.iter().fold(0.0, |acc, x| acc + x.HoursWorked))
        .unwrap_or_default();
    let hours_driven_total = entries
        .as_ref()
        .map(|e| e.iter().fold(0.0, |acc, x| acc + x.HoursDriven))
        .unwrap_or_default();
    let extra_exp_total = entries
        .as_ref()
        .map(|e| e.iter().fold(0, |acc, x| acc + x.ExtraExpCents))
        .unwrap_or_default();

    let totals = WDEntry {
        Date: String::new(),
        Location: String::new(),
        FlatRate: false,
        HoursWorked: hours_worked_total,
        HoursDriven: hours_driven_total,
        ExtraExpCents: extra_exp_total,
    };

    Ok(crate::render(|buf| {
        crate::templates::workerdata_html(
            buf,
            "CZ4R Worker Data",
            admin,
            logged_in,
            worker.worker,
            users.as_slice(),
            selectlist.as_slice(),
            entries,
            totals,
            from,
            to,
        )
    }))
}
