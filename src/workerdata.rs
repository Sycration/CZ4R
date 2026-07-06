use crate::{AppState, Backend, Worker, current_user, now};
use crate::{
    api_auth::ApiAuth,
    errors::{ApiError, CustomError, to_api},
    get_admin,
};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::{Form, Json};
use axum_login::AuthSession;
use axum_template::RenderHtml;
use git_version::git_version;
use serde::{Deserialize, Serialize};
use sqlx::Pool;
use sqlx::types::time::Date;
use time::Time;
use time::format_description::well_known::Iso8601;
use tracing::debug;
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema, utoipa::IntoParams)]
pub(crate) struct WorkerDataQuery {
    pub worker: Option<i64>,
    pub start_date: Option<Date>,
    pub end_date: Option<Date>,
}

#[derive(Debug, Default, Serialize, ToSchema)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct WDEntry {
    pub job_id: i64,
    pub worker_id: i64,
    pub date: String,
    pub location: String,
    pub flat_rate: bool,
    pub hours_worked: String,
    pub true_hours_worked: String,
    pub hours_driven: String,
    pub miles_driven: String,
    pub extra_expenses_dollars: String,
    pub completed: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct WorkerDataOutput {
    pub entries: Vec<WDEntry>,
    pub totals: WDEntry,
    pub num_jobs: usize,
    pub from: String,
    pub to: String,
}

fn hours_worked(signin: Time, signout: Time) -> f32 {
    ((signout - signin).as_seconds_f32() / 3600.).max(1.0)
}

fn fix_neg_zero(val: f32) -> f32 {
    val.abs().copysign(1.0)
}

async fn worker_data_core(
    pool: &Pool<sqlx::Sqlite>,
    user: Option<&crate::CurrentUser>,
    query: &WorkerDataQuery,
) -> Result<WorkerDataOutput, CustomError> {
    let (my_id, my_name) = get_admin(user)?;

    let date = now().date();
    let mut from = String::new();
    let mut to = String::new();

    let (entries, totals) = if let Some(id) = query.worker {
        let start_date = if let Some(d) = query.start_date {
            d
        } else if date.day() <= 15 {
            date.replace_day(1).unwrap()
        } else {
            date.replace_day(16).unwrap()
        };

        let end_date = if let Some(d) = query.end_date {
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
        .fetch_all(pool)
        .await?;

        let hours_worked_total = data
            .iter()
            .filter_map(|d| {
                let signin = Time::parse(d.signin.as_ref()?, &Iso8601::TIME).ok()?;
                let signout = Time::parse(d.signout.as_ref()?, &Iso8601::TIME).ok()?;
                Some(hours_worked(signin, signout))
            })
            .sum::<f32>();

        let true_hours_worked_total = fix_neg_zero(
            data.iter()
                .filter_map(|d| {
                    let signin = Time::parse(d.signin.as_ref()?, &Iso8601::TIME).ok()?;
                    let signout = Time::parse(d.signout.as_ref()?, &Iso8601::TIME).ok()?;
                    Some((signout - signin).as_seconds_f32() / 3600.)
                })
                .sum::<f32>(),
        );

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

        let all_complete = data
            .iter()
            .all(|x| x.signin.is_some() && x.signout.is_some());

        let entries = data
            .into_iter()
            .map(|d| {
                let completed = d.signin.is_some() && d.signout.is_some();
                WDEntry {
                    date: d.date.unwrap(),
                    location: d.sitename,
                    flat_rate: d.using_flat_rate,
                    hours_worked: {
                        if completed {
                            let signin =
                                Time::parse(&d.signin.clone().unwrap(), &Iso8601::TIME).unwrap();
                            let signout =
                                Time::parse(&d.signout.clone().unwrap(), &Iso8601::TIME).unwrap();
                            format!("{:.2}", hours_worked(signin, signout))
                        } else {
                            String::from("N/A")
                        }
                    },
                    true_hours_worked: if completed {
                        let signin = Time::parse(&d.signin.unwrap(), &Iso8601::TIME).unwrap();
                        let signout = Time::parse(&d.signout.unwrap(), &Iso8601::TIME).unwrap();
                        format!(
                            "{:.2}",
                            fix_neg_zero((signout - signin).as_seconds_f32() / 3600.)
                        )
                    } else {
                        String::from("N/A")
                    },
                    hours_driven: format!("{:.2}", d.hours_driven),
                    miles_driven: format!("{:.2}", d.miles_driven),
                    extra_expenses_dollars: format!("{:.2}", (d.extraexpcents as f64 / 100.)),
                    worker_id: d.worker,
                    job_id: d.job,
                    completed,
                }
            })
            .collect::<Vec<_>>();

        let totals = WDEntry {
            date: String::new(),
            location: String::new(),
            flat_rate: false,
            hours_worked: if all_complete {
                format!("{:.2}", hours_worked_total)
            } else {
                String::from("N/A")
            },
            true_hours_worked: format!("{:.2}", true_hours_worked_total),
            hours_driven: format!("{:.2}", hours_driven_total),
            miles_driven: format!("{:.2}", miles_driven_total),
            extra_expenses_dollars: format!("{:.2}", (extra_exp_total as f64 / 100.)),
            job_id: -1,
            worker_id: -1,
            completed: all_complete,
        };

        debug!("admin {my_name} (id {my_id}) retrieved data on user {id} from {from} to {to}");

        (entries, totals)
    } else {
        (vec![], WDEntry::default())
    };

    Ok(WorkerDataOutput {
        num_jobs: entries.len(),
        entries,
        totals,
        from,
        to,
    })
}

/// `GET /admin/worker-data` — HTML-facing endpoint.
pub(crate) async fn workerdatapage(
    State(AppState { pool, engine, .. }): State<AppState>,
    auth: AuthSession<Backend>,
    Form(query): Form<WorkerDataQuery>,
) -> Result<impl IntoResponse, CustomError> {
    let user = current_user(&auth);
    let output = worker_data_core(&pool, user.as_ref(), &query).await?;

    let users = sqlx::query_as!(Worker, "select * from users where deactivated = false;")
        .fetch_all(&pool)
        .await?;

    let selectlist = users
        .iter()
        .map(|w| (w.id, w.name.as_str()))
        .collect::<Vec<_>>();

    let data = serde_json::json!({
        "git_ver": git_version!(),
        "title": "CZ4R Worker Data",
        "admin": true,
        "logged_in": true,
        "selected": query.worker,
        "workerlist": users,
        "selectlist": selectlist,
        "num_jobs": output.num_jobs,
        "entries": output.entries,
        "totals": output.totals,
        "from": output.from,
        "to": output.to,
        "target": "worker-data"
    });

    Ok(RenderHtml("workerdata.hbs", engine, data))
}

/// Returns worker data entries for a given worker and date range.
/// Numbers are strings because the server has specific logic for rounding and formatting them, and the client should not attempt to reformat them.
/// TrueHoursWorked is the actual hours worked, while HoursWorked is the rounded up hours worked (minimum 1 hour).
/// The total object is a WDEntry, but only the numerical fields and the completed field are filled in.
/// The total completed field is true if all entries are completed, false if any entry is incomplete.
#[utoipa::path(
    get,
    path = "/admin/api/v1/worker-data",
    params(WorkerDataQuery),
    responses((status = OK, body = WorkerDataOutput)),
    security(("bearer_auth" = [])),
    tag = super::ADMIN_TAG
)]
pub(crate) async fn workerdatapage_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
    Query(query): Query<WorkerDataQuery>,
) -> Result<Json<WorkerDataOutput>, ApiError> {
    to_api(worker_data_core(&pool, Some(&user), &query).await)
}
