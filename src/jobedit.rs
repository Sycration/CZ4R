use std::collections::HashMap;

use axum::{
    debug_handler,
    extract::State,
    response::{Html, IntoResponse, Redirect},
    Form,
};
use axum_template::RenderHtml;
use itertools::Itertools;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{
    query, query_as, query_builder, types::time::Date, Execute, Pool, QueryBuilder, Sqlite,
};
use std::result::Result::Ok;
use tracing::{info, trace};

use crate::{errors::CustomError, AppState, Job};
use crate::{get_admin, Backend};
use axum_login::AuthSession;
use git_version::git_version;

#[derive(Deserialize)]
pub(crate) struct JobEditPage {
    id: Option<i64>,
}

pub(crate) async fn jobeditpage(
    State(AppState { pool, engine, .. }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<JobEditPage>,
) -> Result<impl IntoResponse, CustomError> {
    get_admin(&auth)?;

    let this_job = match form.id {
        Some(id) => Some(
            query_as!(Job, "select * from jobs where id = $1", id)
                .fetch_one(&pool)
                .await?,
        ),
        None => None,
    };

    let workers = query!("select id, name from users where users.deactivated = false;")
        .fetch_all(&pool)
        .await?
        .into_iter()
        .map(|r| (r.id, r.name))
        .collect::<Vec<_>>();

    let assigned_fr = match form.id {
        Some(id) => query!(
            r#"select users.id, jobworkers.using_flat_rate from users
        inner join jobworkers
        on users.id = jobworkers.worker
        where jobworkers.job = $1
        and users.deactivated = false;
        "#,
            id
        )
        .fetch_all(&pool)
        .await?
        .into_iter()
        .fold(HashMap::new(), |mut acc, x| {
            acc.entry(x.id).or_insert(x.using_flat_rate);
            acc
        }),
        None => HashMap::new(),
    };

    let list_data = workers
        .into_iter()
        .map(|(id, name)| {
            (
                id,
                name,
                assigned_fr.contains_key(&id),
                assigned_fr.get(&id).map_or(false, |v| *v),
            )
        })
        .collect::<Vec<_>>();

    let data = json!({
    "git_ver": git_version!(),
        "title": "Job Edit",
        "admin": true,
        "logged_in": true,
        "job": ({if let Some(job) = this_job {
            json!({
                "id": job.id,
                "sitename": job.sitename,
                "workorder": job.workorder,
                "servicecode": job.servicecode,
                "address": job.address,
                "date": job.date.to_string(),
                "notes": job.notes,
            })
        } else {
            Value::Null
        }}),
        "list-data": list_data
    });

    Ok(RenderHtml("jobedit.hbs", engine, data))
}

#[derive(Deserialize)]
pub(crate) struct JobEditForm {
    sitename: String,
    servcode: String,
    workorder: String,
    address: String,
    date: Date,
    assigned: String,
    flatrate: String,
    jobid: Option<i64>,
    notes: String,
}

pub(crate) async fn jobedit(
    State(AppState { pool, engine, .. }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<JobEditForm>,
) -> Result<impl IntoResponse, CustomError> {
    let (my_id, my_name) = get_admin(&auth)?;

    let to_assign = form
        .assigned
        .split('-')
        .filter_map(|n| n.parse::<i64>().ok())
        .collect::<Vec<_>>();
    let to_flatrt = form
        .flatrate
        .split('-')
        .filter_map(|n| n.parse::<i64>().ok())
        .collect::<Vec<_>>();
    let to_assign = to_assign
        .iter()
        .map(|x| (*x, to_flatrt.contains(x)))
        .collect::<Vec<_>>();

    if let Some(job_id) = form.jobid {
        let mut tx = pool.begin().await?;

        //update job itself
        query!(
            r#"
        update jobs set 
            sitename = $2,
            workorder = $3,
            servicecode = $4,
            address = $5,
            date = $6,
            notes = $7
        where id = $1;"#,
            job_id,
            form.sitename,
            form.workorder,
            form.servcode,
            form.address,
            form.date,
            form.notes
        )
        .execute(&mut *tx)
        .await?;

        let currently_assigned = query!(
            r#"
        select jobworkers.worker, jobworkers.using_flat_rate
            from
        jobworkers inner join jobs
            on jobworkers.job = jobs.id
        where jobs.id = $1;"#,
            job_id
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|v| (v.worker, v.using_flat_rate))
        .collect::<Vec<_>>();

        let flatrates_to_remove = currently_assigned
            .iter()
            .filter(|x| x.1)
            .filter(|x| to_assign.contains(&(x.0, false)))
            .map(|x| x.0)
            .collect::<Vec<_>>();

        let assignments_to_remove = currently_assigned
            .iter()
            .filter(|x| !to_assign.contains(&(x.0, false)) || !to_assign.contains(&(x.0, true)))
            .map(|x| x.0)
            .collect::<Vec<_>>();

        let assignments_to_add = to_assign
            .iter()
            .filter(|x| {
                !currently_assigned.contains(&(x.0, false))
                    || !currently_assigned.contains(&(x.0, true))
            })
            .collect::<Vec<_>>();

        //remove flatrates
        let csl = flatrates_to_remove.iter().join(",");
        let query = QueryBuilder::new("update jobworkers set using_flat_rate = false where job = ")
            .push_bind(job_id)
            .push(" and worker in (")
            .push_bind(csl)
            .push(")")
            .build()
            .execute(&mut *tx)
            .await?;
        trace!(
            "removed flat-rate flags on job {} for users {:?}",
            job_id,
            &flatrates_to_remove
        );

        //remove assignments
        let csl = assignments_to_remove.iter().join(",");
        let query = QueryBuilder::new("delete from jobworkers where job = ")
            .push_bind(job_id)
            .push(" and worker in (")
            .push_bind(csl)
            .push(")")
            .build()
            .execute(&mut *tx)
            .await?;
        trace!(
            "removed assignments on job {} for users {:?}",
            job_id,
            &assignments_to_remove
        );

        //create assignments w/ flatrates
        if !assignments_to_add.is_empty() {
            let mut query_builder: QueryBuilder<Sqlite> =
                QueryBuilder::new("insert into jobworkers (job, worker, using_flat_rate) ");
            query_builder.push_values(assignments_to_add.iter().take(250), |mut b, assignment| {
                b.push_bind(job_id)
                    .push_bind(assignment.0)
                    .push_bind(assignment.1);
            });
            let query = query_builder.build();
            query.execute(&mut *tx).await?;
            trace!(
                "added assignments on job {} for users {:?}\n flat-rates on {:?}",
                job_id,
                &assignments_to_add.iter().map(|x| x.0).collect::<Vec<_>>(),
                &assignments_to_add
                    .iter()
                    .filter(|x| x.1)
                    .map(|x| x.0)
                    .collect::<Vec<_>>(),
            );
        }

        tx.commit().await?;

        info!(
            "admin {my_name} (id {my_id}) updated job {job_id}:\n
site name: {}\n
workorder: {}\n
service code: {}\n
address: {}\n
date: {}\n
notes: {}",
            form.sitename, form.workorder, form.servcode, form.address, form.date, form.notes
        );

        return Ok(Redirect::to(format!("/jobedit?id={}", job_id).as_str()));
    } else {
        let mut tx = pool.begin().await?;

        //create job
        let job_id: i64 = query!(
            r#"
        insert into jobs (sitename, workorder, servicecode, address, date, notes) values
                ($1, $2, $3, $4, $5, $6)
            returning id;"#,
            form.sitename,
            form.workorder,
            form.servcode,
            form.address,
            form.date,
            form.notes
        )
        .fetch_one(&mut *tx)
        .await?
        .id;

        info!(
            "admin {my_name} (id {my_id}) created job {job_id}:\n
site name: {}\n
workorder: {}\n
service code: {}\n
address: {}\n
date: {}\n
notes: {}",
            form.sitename, form.workorder, form.servcode, form.address, form.date, form.notes
        );

        //create assignments w/ flatrates
        if !to_assign.is_empty() {
            let mut query_builder: QueryBuilder<Sqlite> =
                QueryBuilder::new("insert into jobworkers (job, worker, using_flat_rate) ");
            query_builder.push_values(to_assign.iter().take(250), |mut b, assignment| {
                b.push_bind(job_id)
                    .push_bind(assignment.0)
                    .push_bind(assignment.1);
            });

            let query = query_builder.build();
            query.execute(&mut *tx).await?;
        }

        tx.commit().await?;
        return Ok(Redirect::to(format!("/jobedit?id={}", job_id).as_str()));
    }
}

#[derive(Deserialize)]
pub(crate) struct JobDeleteForm {
    jobid: i64,
}

pub(crate) async fn jobdelete(
    State(AppState { pool, engine, .. }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<JobDeleteForm>,
) -> Result<impl IntoResponse, CustomError> {
    let (my_id, my_name) = get_admin(&auth)?;

    query!(
        r#"
    delete from jobworkers
        where 
        job = $1;
    "#,
        form.jobid
    )
    .execute(&pool)
    .await?;

    query!(
        r#"
    delete from jobs
        where 
        id = $1;
    "#,
        form.jobid
    )
    .execute(&pool)
    .await
    .unwrap();

    info!(
        "admin {} (id {}) deleted job {}",
        my_name, my_id, form.jobid
    );

    Ok(Redirect::to("/joblist"))
}
