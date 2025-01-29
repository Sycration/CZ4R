use axum::{
    extract::State,
    response::{Html, IntoResponse, Redirect},
    Form,
};

use axum_template::RenderHtml;
use git_version::git_version;
use password_hash::{rand_core::le, PasswordHasher, Salt, SaltString};
use serde::{Deserialize, Serialize};
use sqlx::types::time::Date;
use sqlx::{query, query_as, Pool};
use time::{
    format_description::well_known::Iso8601, macros::format_description, OffsetDateTime, Time,
};
use tracing::debug;

use crate::{
    errors::{self, CustomError},
    now, AppState, Worker,
};
use crate::{get_admin, Backend};
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
    pub Completed: bool,
}

fn hours_worked(signin: Time, signout: Time) -> f32 {
    ((signout - signin).as_seconds_f32() / 3600.).max(1.0)
}

pub(crate) async fn workerdatapage(
    State(AppState { pool, engine, .. }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(worker): Form<WorkerDataForm>,
) -> Result<impl IntoResponse, CustomError> {
    let (my_id, my_name) = get_admin(&auth)?;

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
        select jobworkers.*, date(jobs.date) as date, jobs.sitename from jobworkers
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
            .map(|x| {
                let signin = Some(Time::parse(&x.signin.clone().unwrap(), &Iso8601::TIME));
                let signout = Some(Time::parse(&x.signout.clone().unwrap(), &Iso8601::TIME));
                (signin, signout)
            })
            .filter(|(signin, signout)| signin.is_some() && signout.is_some())
            .map(|(signin, signout)| (signin.unwrap(), signout.unwrap()))
            .filter(|(signin, signout)| signin.is_ok() && signout.is_ok())
            .map(|(signin, signout)| (signin.unwrap(), signout.unwrap()))
            .fold(0.0, |acc, (signin, signout)| {
                acc + hours_worked(signin, signout)
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

        let all_complete = data.iter().fold(true, |acc, x| {
            if acc {
                x.signin.is_some() && x.signout.is_some()
            } else {
                false
            }
        });

        let entries = data
            .into_iter()
            .map(|d| {
                let Completed = d.signin.is_some() && d.signout.is_some();
                WDEntry {
                    Date: d.date.unwrap(),
                    Location: d.sitename,
                    FlatRate: d.using_flat_rate,
                    HoursWorked: {
                        if Completed {
                            let signin =
                                Time::parse(&d.signin.clone().unwrap(), &Iso8601::TIME).unwrap();
                            let signout =
                                Time::parse(&d.signout.clone().unwrap(), &Iso8601::TIME).unwrap();
                            let val = hours_worked(signin, signout);
                            format!("{:.2}", val)
                        } else {
                            String::from("N/A")
                        }
                    },
                    TrueHoursWorked: if Completed {
                        {
                            let signin = Time::parse(&d.signin.unwrap(), &Iso8601::TIME).unwrap();
                            let signout = Time::parse(&d.signout.unwrap(), &Iso8601::TIME).unwrap();
                            let val = (signout - signin).as_seconds_f32() / 3600.;
                            format!("{:.2}", val)
                        }
                    } else {
                        String::from("N/A")
                    },
                    HoursDriven: format!("{:.2}", d.hours_driven),
                    MilesDriven: format!("{:.2}", d.miles_driven),
                    ExtraExpCents: format!("{:.2}", (d.extraexpcents as f64 / 100.)),
                    WorkerId: d.worker,
                    JobId: d.job,
                    Completed,
                }
            })
            .collect::<Vec<_>>();

        let totals = WDEntry {
            Date: String::new(),
            Location: String::new(),
            FlatRate: false,
            HoursWorked: if all_complete {
                format!("{:.2}", hours_worked_total)
            } else {
                String::from("N/A")
            },
            TrueHoursWorked: String::new(),
            HoursDriven: format!("{:.2}", hours_driven_total),
            MilesDriven: format!("{:.2}", miles_driven_total),
            ExtraExpCents: format!("{:.2}", (extra_exp_total as f64 / 100.)),
            JobId: -1,
            WorkerId: -1,
            Completed: all_complete,
        };

        let user = selectlist.iter().find(|u| u.0 == id).unwrap();
        debug!(
            "admin {my_name} (id {my_id}) retrieved data on user {} (id {}) from {} to {}",
            user.1, id, from, to
        );

        (entries, totals)
    } else {
        (vec![], WDEntry::default())
    };

    let data = serde_json::json!({
    "git_ver": git_version!(),
        "title": "CZ4R Worker Data",
        "admin": true,
        "logged_in": true,
        "selected": worker.worker,
        "workerlist": users,
        "selectlist": selectlist,
        "num_jobs": entries.len(),
        "entries": entries,
        "totals": totals,
        "from": &from,
        "to": &to,
        "target": "worker-data"
    });

    Ok(RenderHtml("workerdata.hbs", engine, data))
}
