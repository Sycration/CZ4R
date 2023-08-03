use std::collections::BTreeMap;

use axum::{
    extract::State,
    response::{Html, IntoResponse},
    Form,
};
use axum_template::RenderHtml;
use serde::{Deserialize, Serialize};
use sqlx::{
    query, query_as, query_builder, types::time::Date, Execute, FromRow, Pool, Postgres,
    QueryBuilder,
};
use time::{Duration, OffsetDateTime, Time};

use crate::{empty_string_as_none, errors::CustomError, now, AppState, Auth, TZ_OFFSET};

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
    pub status: String,
}

impl JobData {
    fn from_outputs(jobs: Vec<JobQueryOutput>) -> Vec<Self> {
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
            .collect::<Vec<_>>()
    }
}

#[derive(Deserialize)]
pub(crate) struct JobListPage {
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
    mut auth: Auth,
    Form(form): Form<JobListPage>,
) -> Result<impl IntoResponse, CustomError> {
    let admin = auth.current_user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.current_user.is_some();

    if !logged_in {
        return Err(CustomError::Auth("Not logged in".to_string()));
    }

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

    let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
        r#"select users.name, jobs.id, jobworkers.worker, 
        jobworkers.notes as workernotes, jobworkers.signin, 
        jobworkers.miles_driven, jobworkers.hours_driven,
        jobworkers.extraexpcents, jobworkers.signout, jobs.sitename, jobs.address, 
        jobs.date, jobs.notes, jobs.workorder 
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
        query_builder.push_bind(auth.current_user.as_ref().unwrap().id);
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

    query_builder.push(" order by date desc;");

    let query = query_builder.build_query_as();

    let query = query.fetch_all(&pool).await;

    let jobs = match query {
        Ok(mut r) => {
            let query = query_as!(
                JobQueryOutput,
                r#"
            select '' as "name!", NULL::bigint as worker, jobs.id,
            jobs.sitename, jobs.address, jobs.date, NULL::time as signin, 
            NULL::time as signout, NULL::varchar as workernotes,
            jobs.notes, jobs.workorder, NULL::real as miles_driven,
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
        }
        Err(e) => return Err(CustomError::Database(e.to_string())),
    };

    dbg!(admin);
    let data = serde_json::json!({
        "title": "CZ4R Job List",
        "admin": admin,
        "logged_in": logged_in,
        "job_datas": JobData::from_outputs(jobs),
        "params": SearchParams {
            start: start_date.to_string(),
            end: end_date.to_string(),
            site_name: form.site_name.unwrap_or_default(),
            work_order: form.work_order.unwrap_or_default(),
            address: form.address.unwrap_or_default(),
            fieldnotes: form.notes.unwrap_or_default(),
        }
    });

    Ok(RenderHtml("joblist.hbs", engine, data))
}
