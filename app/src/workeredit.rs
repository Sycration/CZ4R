use super::render;
use crate::errors::CustomError;
use axum::response::Html;
use super::WorkerEditForm;
use axum::Form;
use super::Auth;
use super::Worker;
use sqlx::Postgres;
use sqlx::Pool;
use axum::extract::State;

pub(crate) async fn workeredit(
    State(pool): State<Pool<Postgres>>, mut auth: Auth,
    Form(worker): Form<WorkerEditForm>,
) -> Result<Html<String>, CustomError> {
    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
     let admin = auth.current_user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.current_user.is_some();

    if admin && logged_in {
        let users = sqlx::query_as!(Worker, "
        select * from users
        ").fetch_all(&pool).await.unwrap();
        let selectlist = users.iter().map(|w| (w.id, w.name.as_str())).collect::<Vec<_>>();


        Ok(crate::render(|buf| {
            crate::templates::workeredit_html(buf, "CZ4R Worker Edit", admin, (worker.creating==Some(true)), worker.worker,users.as_slice(), selectlist.as_slice())
        }))
    } else if logged_in {
        Err(CustomError::Auth("Not Admin".to_string()))
    } else {
        Err(CustomError::Auth("Not Logged In".to_string()))
    }
    
}
