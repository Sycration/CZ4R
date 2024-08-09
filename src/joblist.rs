use std::collections::BTreeMap;

use crate::{get_user, Backend};
use crate::{empty_string_as_none, errors::CustomError, now, AppState, TZ_OFFSET};
use axum::{
    extract::State,
    response::{Html, IntoResponse},
    Form,
};
use axum_login::AuthSession;
use axum_template::RenderHtml;
use serde::{Deserialize, Serialize};
use sqlx::{
    query, query_as, query_builder, types::time::Date, Execute, FromRow, Pool, Postgres,
    QueryBuilder,
};
use time::{Duration, OffsetDateTime, Time};

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
    signin: Option<Time>,
    signout: Option<Time>,
    workernotes: Option<String>,
    miles_driven: Option<f32>,
    hours_driven: Option<f32>,
    extraexpcents: Option<i32>,
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
}

pub(crate) async fn joblistpage(
    State(AppState { pool, engine }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<JobListForm>,
) -> Result<impl IntoResponse, CustomError> {
    let (id, admin) = get_user(auth)?;

    let start_date = if let Some(d) = form.start_date {
        d
    } else {
        now().date()
    };
    let end_date = if let Some(d) = form.end_date {
        d
    } else {
        (now() + Duration::days(15)).date()
    };

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

    let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
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

    query_builder.push("where date >= ");
    query_builder.push_bind(start_date);
    query_builder.push(" and date <= ");
    query_builder.push_bind(end_date);

    if !admin {
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
            select '' as "name!", NULL::bigint as worker, jobs.id,
            jobs.sitename, jobs.address, jobs.date, NULL::time as signin, 
            NULL::time as signout, NULL::varchar as workernotes,
            jobs.notes, jobs.workorder, jobs.servicecode, NULL::real as miles_driven,
            NULL::real as hours_driven, NULL::integer as extraexpcents from jobs 

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
                    orphans.append(&mut r);
                    orphans
                }
            }
            r
        };

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
        },
        "order": form.order.unwrap_or(Order::Latest),
        "assigned": assigned,
        "started": started,
        "completed": completed
    });

    Ok(RenderHtml("joblist.hbs", engine, data))
}
