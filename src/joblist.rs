use std::collections::BTreeMap;

use crate::api_auth::ApiAuth;
use crate::errors::{ApiError, to_api};
use crate::{AppState, TZ_OFFSET, current_user, empty_string_as_none, errors::CustomError, now};
use crate::{Backend, get_user};
use axum::{
    Form, Json,
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use axum_login::AuthSession;
use axum_login::tower_sessions::Session;
use axum_template::RenderHtml;
use git_version::git_version;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use sqlx::Sqlite;
use sqlx::{
    Execute, FromRow, Pool, QueryBuilder, query, query_as, query_builder, types::time::Date,
};
use time::{Duration, OffsetDateTime, Time};
use tracing::warn;
use utoipa::{IntoParams, ToSchema};

#[derive(Deserialize, FromRow)]
struct JobQueryOutput {
    name: String,
    id: i64,
    worker: Option<i64>,
    sitename: String,
    address: String,
    date: time::Date,
    notes: String,
    workorder: String,
    servicecode: String,
    signin: Option<String>,
    signout: Option<String>,
    workernotes: Option<String>,
    miles_driven: Option<f64>,
    hours_driven: Option<f64>,
    extraexpcents: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Assigned,
    Started,
    SignedOut,
    OutNotIn,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct JobData {
    pub job_id: i64,
    pub worker_id: Option<i64>,
    pub worker_name: String,
    pub job_name: String,
    pub address: String,
    pub date: String,
    pub notes: String,
    pub work_order: String,
    pub service_code: String,
    pub status: Status,
}

impl JobData {
    fn from_outputs(
        jobs: Vec<JobQueryOutput>,
        assigned: bool,
        started: bool,
        completed: bool,
    ) -> Vec<Self> {
        jobs.into_iter()
            .map(|j| JobData {
                job_id: j.id,
                worker_id: j.worker,
                worker_name: j.name,
                job_name: j.sitename,
                address: j.address,
                date: format!("{} {}, {}", j.date.month(), j.date.day(), j.date.year()),
                notes: j.notes,
                work_order: j.workorder,
                service_code: j.servicecode,
                status: {
                    match (j.signin, j.signout) {
                        (None, None) => {
                            if j.hours_driven.map(|x| x == 0.) != Some(true)
                                || j.miles_driven.map(|x| x == 0.) != Some(true)
                                || j.extraexpcents.map(|x| x == 0) != Some(true)
                                || j.workernotes.map(|x| x.is_empty()) != Some(true)
                            {
                                Status::Started
                            } else {
                                Status::Assigned
                            }
                        }
                        (None, Some(_)) => Status::OutNotIn,
                        (Some(_), None) => Status::Started,
                        (Some(_), Some(_)) => Status::SignedOut,
                    }
                },
            })
            .filter(|d| {
                (assigned && d.status == Status::Assigned)
                    || (started && d.status == Status::Started)
                    || (completed && d.status == Status::SignedOut)
                    || d.status == Status::OutNotIn
            })
            .collect::<Vec<_>>()
    }
}

#[derive(Debug, Clone, Deserialize, IntoParams)]
pub(crate) struct JobListForm {
    start_date: Option<Date>,
    end_date: Option<Date>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub site_name: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub work_order: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub address: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub notes: Option<String>,
    pub order: Option<Order>,
    pub assigned: Option<bool>,
    pub started: Option<bool>,
    pub completed: Option<bool>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub workers: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub enum Order {
    Latest,
    Earliest,
}

/// A worker, and whether they're currently included in a job-list search's
/// worker filter.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WorkerFilterOption {
    pub id: i64,
    pub name: String,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SearchParams {
    pub start: String,
    pub end: String,
    pub site_name: String,
    pub work_order: String,
    pub address: String,
    pub fieldnotes: String,
    pub workers: Vec<WorkerFilterOption>,
}

/// The full result of a job-list search: every matching job/assignment row
/// plus the (possibly defaulted) search parameters that produced it. Shared
/// by both the HTML page and the JSON API.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct JobListOutput {
    pub admin: bool,
    pub count: usize,
    pub jobs: Vec<JobData>,
    pub params: SearchParams,
    pub order: Order,
    pub assigned: bool,
    pub started: bool,
    pub completed: bool,
}

async fn joblist_core(
    pool: &Pool<Sqlite>,
    user: Option<&crate::CurrentUser>,
    form: JobListForm,
) -> Result<JobListOutput, CustomError> {
    let (id, _my_name, admin) = get_user(user)?;

    let start_date = if let Some(d) = form.start_date {
        d
    } else {
        now().date()
    }
    .to_string();
    let end_date = if let Some(d) = form.end_date {
        d
    } else {
        (now() + Duration::days(15)).date()
    }
    .to_string();

    //testing form.order because that is always sent on form submit
    let assigned = form.assigned.unwrap_or(true);
    let started = form.started.unwrap_or(true);
    let completed = form.completed.unwrap_or(true);

    let parsed_workers = if let Some(w) = &form.workers {
        w.split('-').filter_map(|x| x.parse::<i64>().ok()).collect()
    } else {
        vec![]
    };

    let mut query_builder: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"select users.name, jobs.id, jobworkers.worker, 
        jobworkers.notes as workernotes, jobworkers.signin, 
        jobworkers.miles_driven, jobworkers.hours_driven,
        jobworkers.extraexpcents, jobworkers.signout, jobs.sitename, jobs.address, 
        jobs.date, jobs.notes, jobs.workorder, jobs.servicecode
        from jobs inner join jobworkers
                on jobs.id = jobworkers.job
                inner join users
                on jobworkers.worker = users.id
                 "#,
    );

    query_builder.push("where date(jobs.date) >= ");
    query_builder.push_bind(&start_date);
    query_builder.push(" and date(jobs.date) <= ");
    query_builder.push_bind(&end_date);

    if admin && form.workers.is_some() {
        query_builder.push(" and jobworkers.worker in (");
        for (idx, id) in parsed_workers.iter().enumerate() {
            query_builder.push_bind(id);
            if idx != parsed_workers.len() - 1 {
                query_builder.push(',');
            }
        }
        query_builder.push(") ");
    } else if admin && form.workers.is_none() {
    } else {
        query_builder.push(" and jobworkers.worker = ");
        query_builder.push_bind(id);
    }

    if let Some(site_name) = &form.site_name {
        query_builder.push(" and jobs.sitename like concat('%', ");
        query_builder.push_bind(site_name);
        query_builder.push(", '%') ");
    }

    if let Some(work_order) = &form.work_order {
        query_builder.push("and jobs.workorder like concat('%', ");
        query_builder.push_bind(work_order);
        query_builder.push(", '%') ");
    }

    if let Some(address) = &form.address {
        query_builder.push("and jobs.address like concat('%', ");
        query_builder.push_bind(address);
        query_builder.push(", '%') ");
    }

    if let Some(notes) = &form.notes {
        query_builder.push("and jobworkers.notes like concat('%', ");
        query_builder.push_bind(notes);
        query_builder.push(", '%') ");
    }

    match form.order {
        Some(Order::Earliest) => {
            query_builder.push(" order by date asc;");
        }
        _ => {
            query_builder.push(" order by date desc;");
        }
    }

    let query = query_builder.build_query_as();

    let mut r = query.fetch_all(pool).await?;

    let jobs = {
        let query = query_as!(
            JobQueryOutput,
            r#"
            select '' as "name!", 0 as worker, jobs.id,
            jobs.sitename, jobs.address, jobs.date, time(0) as signin, 
            time(0) as signout, '' as workernotes,
            jobs.notes, jobs.workorder, jobs.servicecode, 0.0 as miles_driven,
            0.0 as hours_driven, 0 as extraexpcents from jobs 

            where not exists (
                select *
                from jobworkers
                where jobworkers.job = jobs.id
            )
            and date >= $1 and date <= $2
            order by date desc;
            "#,
            start_date,
            end_date
        )
        .fetch_all(pool)
        .await;
        if let Ok(mut orphans) = query {
            r = {
                if !orphans.is_empty() {
                    warn!(
                        "orphan jobs returned in search: {:?}",
                        orphans.iter().map(|j| j.id).collect::<Vec<_>>()
                    );
                }
                orphans.append(&mut r);
                orphans
            }
        }
        r
    };

    let workers = query!(
        r#"
            select id, name from users where users.deactivated = false
        "#
    )
    .fetch_all(pool)
    .await?
    .iter()
    .map(|w| WorkerFilterOption {
        id: w.id,
        name: w.name.clone(),
        selected: if form.workers.is_some() {
            parsed_workers.contains(&w.id)
        } else {
            true
        },
    })
    .collect();

    let job_datas = JobData::from_outputs(jobs, assigned, started, completed);

    Ok(JobListOutput {
        admin,
        count: job_datas.len(),
        jobs: job_datas,
        params: SearchParams {
            start: start_date,
            end: end_date,
            site_name: form.site_name.unwrap_or_default(),
            work_order: form.work_order.unwrap_or_default(),
            address: form.address.unwrap_or_default(),
            fieldnotes: form.notes.unwrap_or_default(),
            workers,
        },
        order: form.order.unwrap_or(Order::Latest),
        assigned,
        started,
        completed,
    })
}

/// `GET /joblist` — HTML-facing endpoint.
pub(crate) async fn joblistpage(
    State(AppState { pool, engine, .. }): State<AppState>,
    auth: AuthSession<Backend>,
    Form(form): Form<JobListForm>,
) -> Result<impl IntoResponse, CustomError> {
    let out = joblist_core(&pool, current_user(&auth).as_ref(), form).await?;

    let data = serde_json::json!({
    "git_ver": git_version!(),
        "title": "CZ4R Job List",
        "admin": out.admin,
        "logged_in": true,
        "count": out.count,
        "job_datas": out.jobs,
        "params": out.params,
        "order": out.order,
        "assigned": out.assigned,
        "started": out.started,
        "completed": out.completed
    });

    Ok(RenderHtml("joblist.hbs", engine, data))
}

/// The workers parameter is a dash-separated list of worker ids to filter by, e.g. `workers=1-2-3`.
/// Date range is YYYY-MM-DD format, defaults to today through 15 days from now.
/// Non-admins can only see their own jobs, and the workers parameter is ignored for them.
/// Admins' view defaults to show all assignments for all workers, which is the recommended default.
/// The three booleans default to true
#[utoipa::path(
    get,
    path = "/api/v1/joblist",
    params(JobListForm),
    responses((status = OK, body = JobListOutput)),
    security(("bearer_auth" = [])),
    tag = super::USER_TAG
)]
pub(crate) async fn joblist_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
    Query(form): Query<JobListForm>,
) -> Result<Json<JobListOutput>, ApiError> {
    to_api(joblist_core(&pool, Some(&user), form).await)
}
