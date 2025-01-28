use std::collections::BTreeMap;

use crate::{empty_string_as_none, errors::CustomError, now, AppState, TZ_OFFSET};
use crate::{get_user, Backend};
use axum::{
    extract::State,
    response::{Html, IntoResponse},
    Form,
};
use axum_login::tower_sessions::Session;
use axum_login::AuthSession;
use axum_template::RenderHtml;
use itertools::Itertools;
use password_hash::rand_core::le;
use serde::{Deserialize, Serialize};
use sqlx::Sqlite;
use sqlx::{
    query, query_as, query_builder, types::time::Date, Execute, FromRow, Pool, QueryBuilder,
};
use time::{Duration, OffsetDateTime, Time};
use tracing::warn;

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

#[derive(Serialize, Deserialize, FromRow)]
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
    pub status: String,
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
                                "started".to_owned()
                            } else {
                                "assigned".to_owned()
                            }
                        }
                        (None, Some(_)) => "outnotin".to_owned(),
                        (Some(_), None) => "started".to_owned(),
                        (Some(_), Some(_)) => "signedout".to_owned(),
                    }
                },
            })
            .filter(|d| {
                (assigned && d.status.starts_with('a'))
                    || (started && d.status.starts_with("st"))
                    || (completed && d.status.starts_with("si"))
                    || d.status.starts_with('o')
            })
            .collect::<Vec<_>>()
    }
}

#[derive(Deserialize)]
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

#[derive(Serialize, Deserialize, Debug)]
pub enum Order {
    Latest,
    Earliest,
}

#[derive(Deserialize, Serialize)]
pub struct SearchParams {
    pub start: String,
    pub end: String,
    pub site_name: String,
    pub work_order: String,
    pub address: String,
    pub fieldnotes: String,
    pub workers: Vec<(i64, String, bool)>,
}

pub(crate) async fn joblistpage(
    State(AppState { pool, engine, .. }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<JobListForm>,
) -> Result<impl IntoResponse, CustomError> {
    let (id, _my_name, admin) = get_user(&auth)?;

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
    let assigned = if form.order.is_some() {
        form.assigned.unwrap_or(false)
    } else {
        true
    };
    let started = if form.order.is_some() {
        form.started.unwrap_or(false)
    } else {
        true
    };
    let completed = if form.order.is_some() {
        form.completed.unwrap_or(false)
    } else {
        true
    };

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
    } else {
        query_builder.push(" and jobworkers.worker = ");
        query_builder.push_bind(id);
    }

    if let Some(site_name) = &form.site_name {
        query_builder.push(" and jobs.sitename ilike concat('%', ");
        query_builder.push_bind(site_name);
        query_builder.push(", '%') ");
    }

    if let Some(work_order) = &form.work_order {
        query_builder.push("and jobs.workorder ilike concat('%', ");
        query_builder.push_bind(work_order);
        query_builder.push(", '%') ");
    }

    if let Some(address) = &form.address {
        query_builder.push("and jobs.address ilike concat('%', ");
        query_builder.push_bind(address);
        query_builder.push(", '%') ");
    }

    if let Some(notes) = &form.notes {
        query_builder.push("and jobworkers.notes ilike concat('%', ");
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

    let mut r = query.fetch_all(&pool).await?;

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
        .fetch_all(&pool)
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
    .fetch_all(&pool)
    .await?
    .iter()
    .map(|w| {
        (
            w.id,
            w.name.clone(),
            if form.workers.is_some() {
                parsed_workers.contains(&w.id)
            } else {
                true
            },
        )
    })
    .collect();

    let job_datas = JobData::from_outputs(jobs, assigned, started, completed);
    let data = serde_json::json!({
        "title": "CZ4R Job List",
        "admin": admin,
        "logged_in": true,
        "count": &job_datas.len(),
        "job_datas": job_datas,
        "params": SearchParams {
            start: start_date.to_string(),
            end: end_date.to_string(),
            site_name: form.site_name.unwrap_or_default(),
            work_order: form.work_order.unwrap_or_default(),
            address: form.address.unwrap_or_default(),
            fieldnotes: form.notes.unwrap_or_default(),
            workers
        },
        "order": form.order.unwrap_or(Order::Latest),
        "assigned": assigned,
        "started": started,
        "completed": completed
    });

    Ok(RenderHtml("joblist.hbs", engine, data))
}
