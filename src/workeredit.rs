use super::Auth;
use super::Worker;
use crate::AppState;
use crate::errors::CustomError;
use axum::extract::State;
use axum_template::RenderHtml;
use serde_json::json;
use crate::IntoResponse;
use axum::response::Html;
use axum::Form;
use serde::Deserialize;
use sqlx::Pool;
use sqlx::Postgres;

#[derive(Deserialize)]
pub(crate) struct WorkerEditForm {
    worker: Option<i64>,
    creating: Option<bool>,
}

pub(crate) async fn workeredit(
    State(AppState { pool, engine }): State<AppState>,
    mut auth: Auth,
    Form(worker): Form<WorkerEditForm>,
) -> Result<impl IntoResponse, CustomError> {
    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = auth.current_user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.current_user.is_some();

    if admin && logged_in {
        let users = sqlx::query_as!(
            Worker,
            "
        select * from users where deactivated = false;
        "
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let selectlist = users
            .iter()
            .map(|w| (w.id, w.name.as_str()))
            .collect::<Vec<_>>();

        let data = serde_json::json!({
            "admin": admin,
            "logged_in": logged_in,
            "title": "CZ4R Error 404",
            "target": "worker-edit",
            "creating": worker.creating == Some(true),
            "selected": worker.worker,
            "selectlist": selectlist,
            "own_id": auth.current_user.unwrap().id,
            "workerlist": (users.iter().map(|u|json!({
                "id": u.id,
                "name": u.name,
                "hash": u.hash,
                "salt": u.salt,
                "admin": u.admin,
                "address": u.address,
                "phone": u.phone,
                "email": u.email,
                "rate_hourly_cents": format!("{:.2}", (u.rate_hourly_cents as f64 / 100.)),
                "rate_mileage_cents": format!("{:.2}", (u.rate_mileage_cents as f64 / 100.)),
                "rate_drive_hourly_cents": format!("{:.2}", (u.rate_drive_hourly_cents as f64 / 100.)),
                "flat_rate_cents": format!("{:.2}", (u.flat_rate_cents as f64 / 100.)),
                "must_change_pw": u.must_change_pw
            })).collect::<Vec<_>>())
        });

        Ok(RenderHtml("workeredit.hbs",engine,data))
    } else if logged_in {
        Err(CustomError::Auth("Not Admin".to_string()))
    } else {
        Err(CustomError::Auth("Not Logged In".to_string()))
    }
}
