use axum::{extract::State, response::Html, Form};
use serde::Deserialize;
use sqlx::{query, query_as, types::time::Date, Pool, Postgres};
use time::{Duration, OffsetDateTime};

use crate::{errors::CustomError, now, Auth, TZ_OFFSET};

#[derive(Deserialize)]
struct JobCard {
    name: String,
    id: i64,
    worker: Option<i64>,
    sitename: String,
    address: String,
    date: time::Date,
    notes: String,

}

#[derive(Deserialize)]
pub(crate) struct JobListPage {
    start_date: Option<Date>,
    end_date: Option<Date>,
}

pub(crate) async fn joblistpage(
    State(pool): State<Pool<Postgres>>,
    mut auth: Auth,
    Form(form): Form<JobListPage>,
) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
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

    let jobs = match admin {
        true => {
            let query=query_as!(JobCard, r#"
        select users.name, jobs.id, jobworkers.worker as "worker?", jobs.sitename, jobs.address, jobs.date, jobs.notes from jobs inner join jobworkers
        on jobs.id = jobworkers.job
        inner join users
        on jobworkers.worker = users.id
        where date >= $1 and date <= $2
        order by date desc;
        "#, start_date, end_date).fetch_all(&pool).await;
            match query {
                Ok(mut r) => {
                    let query =query_as!(JobCard, r#"
                    select '' as "name!", NULL::bigint as worker, jobs.id, jobs.sitename, jobs.address, jobs.date, jobs.notes from jobs
                    where not exists (
                        select *
                        from jobworkers
                        where jobworkers.job = jobs.id
                    )
                    and date >= $1 and date <= $2
                    order by date desc;
                    "#, start_date, end_date).fetch_all(&pool).await;
                    if let Ok(mut orphans) = query {
                        r = {
                            orphans.append(&mut r);
                            orphans
                        }
                    }
                    r
                },
                Err(e) => return Err(CustomError::Database(e.to_string())),
            }
        }

        false => {
            let query=query_as!(JobCard, r#"
        select users.name, jobs.id,  jobworkers.worker as "worker?", jobs.sitename, jobs.address, jobs.date, jobs.notes from jobs inner join jobworkers
        on jobs.id = jobworkers.job
        inner join users
        on jobworkers.worker = users.id
        where date >= $2 and date <= $3
        and jobworkers.worker = $1
        order by date desc;
        "#, auth.current_user.unwrap().id,  start_date, end_date).fetch_all(&pool).await;
            match query {
                Ok(r) => r,
                Err(e) => return Err(CustomError::Database(e.to_string())),
            }
        }
    };

    let job_datas = jobs
        .into_iter()
        .map(|j| {
            (
                j.id,
                j.worker,
                j.name,
                j.sitename,
                j.address,
                format!("{} {}, {}", j.date.month(), j.date.day(), j.date.year()),
                j.notes
            )
        })
        .collect::<Vec<_>>();

    Ok(crate::render(|buf| {
        crate::templates::joblist_html(
            buf,
            "CZ4R Job List",
            admin,
            logged_in,
            job_datas.as_slice(),
            start_date.to_string(),
            end_date.to_string(),
        )
    }))
}
