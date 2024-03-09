use std::collections::HashMap;

use axum::{
    debug_handler,
    extract::State,
    response::{Html, IntoResponse, Redirect},
    Form,
};
use axum_template::RenderHtml;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{
    query, query_as, query_builder, types::time::Date, Execute, Pool, Postgres, QueryBuilder,
};

use crate::Backend;
use crate::{errors::CustomError, AppState, Job};
use axum_login::AuthSession;
#[derive(Deserialize)]
pub(crate) struct JobEditPage {
    id: Option<i64>,
}

pub(crate) async fn jobeditpage(
    State(AppState { pool, engine }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<JobEditPage>,
) -> Result<impl IntoResponse, CustomError> {
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.user.is_some();

    if !admin {
        return Err(CustomError::Auth("Not logged in as admin".to_string()));
    }

    let this_job = match form.id {
        Some(id) => match query_as!(Job, "select * from jobs where id = $1", id)
            .fetch_one(&pool)
            .await
        {
            Ok(r) => Some(r),
            Err(e) => return Err(CustomError::Database(e.to_string())),
        },
        None => None,
    };

    let workers = match query!("select id, name from users where users.deactivated = false;")
        .fetch_all(&pool)
        .await
    {
        Ok(r) => r.into_iter().map(|r| (r.id, r.name)).collect::<Vec<_>>(),
        Err(e) => return Err(CustomError::Database(e.to_string())),
    };

    let assigned_fr = match form.id {
        Some(id) => match query!(
            r#"select users.id, jobworkers.using_flat_rate from users
        inner join jobworkers
        on users.id = jobworkers.worker
        where jobworkers.job = $1
        and users.deactivated = false;
        "#,
            id
        )
        .fetch_all(&pool)
        .await
        {
            Ok(r) => r.into_iter().fold(HashMap::new(), |mut acc, x| {
                acc.entry(x.id).or_insert(x.using_flat_rate);
                acc
            }),
            Err(e) => return Err(CustomError::Database(e.to_string())),
        },
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
        "title": "Job Edit",
        "admin": admin,
        "logged_in": logged_in,
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
    State(pool): State<Pool<Postgres>>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<JobEditForm>,
) -> Result<Redirect, CustomError> {
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);

    if !admin {
        return Err(CustomError::Auth("Not logged in as admin".to_string()));
    }

    let to_assign = form
        .assigned
        .split('-')
        .filter_map(|n| i64::from_str_radix(n, 10).ok())
        .collect::<Vec<_>>();
    let to_flatrt = form
        .flatrate
        .split('-')
        .filter_map(|n| i64::from_str_radix(n, 10).ok())
        .collect::<Vec<_>>();
    let to_assign = to_assign
        .iter()
        .map(|x| (*x, to_flatrt.contains(x)))
        .collect::<Vec<_>>();

    if let Some(job_id) = form.jobid {
        let mut tx = match pool.begin().await {
            Ok(tx) => tx,
            Err(e) => return Err(CustomError::Database(e.to_string())),
        };

        //update job itself
        let query = query!(
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
        .await;
        if let Err(e) = query {
            return Err(CustomError::Database(e.to_string()));
        }

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
        .await;
        let currently_assigned = match currently_assigned {
            Ok(v) => v
                .into_iter()
                .map(|v| (v.worker, v.using_flat_rate))
                .collect::<Vec<_>>(),
            Err(e) => return Err(CustomError::Database(e.to_string())),
        };

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
        if !flatrates_to_remove.is_empty() {
            let query = query!("update jobworkers set using_flat_rate = false where job = $1 and worker = any($2);",job_id, flatrates_to_remove.as_slice()).execute(&mut *tx).await;
            if let Err(e) = query {
                return Err(CustomError::Database(e.to_string()));
            }
        }

        //remove assignments
        if !assignments_to_remove.is_empty() {
            let query = query!(
                "delete from jobworkers where job = $1 and worker = any($2);",
                job_id,
                assignments_to_remove.as_slice()
            )
            .execute(&mut *tx)
            .await;
            if let Err(e) = query {
                return Err(CustomError::Database(e.to_string()));
            }
        }

        //create assignments w/ flatrates
        if !assignments_to_add.is_empty() {
            let mut query_builder: QueryBuilder<Postgres> =
                QueryBuilder::new("insert into jobworkers (job, worker, using_flat_rate) ");
            query_builder.push_values(assignments_to_add.iter().take(250), |mut b, assignment| {
                b.push_bind(job_id)
                    .push_bind(assignment.0)
                    .push_bind(assignment.1);
            });

            let query = query_builder.build();
            let query = query.execute(&mut *tx).await;
            if let Err(e) = query {
                return Err(CustomError::Database(e.to_string()));
            }
        }

        let res = tx.commit().await;
        if let Err(e) = res {
            return Err(CustomError::Database(e.to_string()));
        }

        return Ok(Redirect::to(format!("/jobedit?id={}", job_id).as_str()));
    } else {
        let mut tx = match pool.begin().await {
            Ok(tx) => tx,
            Err(e) => return Err(CustomError::Database(e.to_string())),
        };

        //update job itself
        let query = query!(
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
        .await;
        let job_id = match query {
            Ok(v) => v.id,
            Err(e) => {
                return Err(CustomError::Database(e.to_string()));
            }
        };

        //create assignments w/ flatrates
        if !to_assign.is_empty() {
            let mut query_builder: QueryBuilder<Postgres> =
                QueryBuilder::new("insert into jobworkers (job, worker, using_flat_rate) ");
            query_builder.push_values(to_assign.iter().take(250), |mut b, assignment| {
                b.push_bind(job_id)
                    .push_bind(assignment.0)
                    .push_bind(assignment.1);
            });

            let query = query_builder.build();
            let query = query.execute(&mut *tx).await;
            if let Err(e) = query {
                return Err(CustomError::Database(e.to_string()));
            }
        }

        let res = tx.commit().await;
        if let Err(e) = res {
            return Err(CustomError::Database(e.to_string()));
        }

        return Ok(Redirect::to(format!("/jobedit?id={}", job_id).as_str()));
    }
}

#[derive(Deserialize)]
pub(crate) struct JobDeleteForm {
    jobid: i64,
}

pub(crate) async fn jobdelete(
    State(pool): State<Pool<Postgres>>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<JobDeleteForm>,
) -> Result<Redirect, CustomError> {
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);

    if !admin {
        return Err(CustomError::Auth("Not logged in as admin".to_string()));
    }

    query!(
        r#"
    delete from jobworkers
        where 
        job = $1;
    "#,
        form.jobid
    )
    .execute(&pool)
    .await
    .unwrap();

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

    Ok(Redirect::to("/joblist"))
}
