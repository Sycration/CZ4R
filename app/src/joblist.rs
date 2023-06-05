use axum::{extract::State, response::Html};
use serde::Deserialize;
use sqlx::{query, types::time::Date, Pool, Postgres, query_as};

use crate::{errors::CustomError, Auth};

#[derive(Deserialize)]
struct JobCard {
    name: String, 
    id: i64,
    sitename: String,
    address: String, 
    date: time::Date
}

pub(crate) async fn joblistpage(
    State(pool): State<Pool<Postgres>>,
    mut auth: Auth,
) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = auth.current_user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.current_user.is_some();

    if !logged_in {
        return Err(CustomError::Auth("Not logged in".to_string()));
    }

    let jobs = match admin {
        true => {let query=query_as!(JobCard, r#"
        select users.name, jobs.id, jobs.sitename, jobs.address, jobs.date from jobs inner join jobworkers
        on jobs.id = jobworkers.job
        inner join users
        on jobworkers.worker = users.id
        where date >  CAST(NOW() AS date) - 1
        order by date desc;
        "#,).fetch_all(&pool).await;
        match query {
            Ok(r) => r,
            Err(e) => return Err(CustomError::Database(e.to_string())),
        }
    },

        false => {let query=query_as!(JobCard, r#"
        select users.name, jobs.id, jobs.sitename, jobs.address, jobs.date from jobs inner join jobworkers
        on jobs.id = jobworkers.job
        inner join users
        on jobworkers.worker = users.id
        where date >  CAST(NOW() AS date) - 1
        and jobworkers.worker = $1
        order by date desc;
        "#, auth.current_user.unwrap().id).fetch_all(&pool).await;
        match query {
            Ok(r) => r,
            Err(e) => return Err(CustomError::Database(e.to_string())),
        }
    },
    };

    let job_datas = jobs.into_iter().map(|j| (j.id, j.name, j.sitename, j.address, j.date.to_string())).collect::<Vec<_>>();


    
    Ok(crate::render(|buf| {
        crate::templates::joblist_html(buf, "CZ4R Job List", admin, logged_in, job_datas.as_slice())
    }))
}
