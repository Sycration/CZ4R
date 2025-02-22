use super::Worker;
use crate::errors::CustomError;
use crate::get_admin;
use crate::AppState;
use crate::Backend;
use crate::IntoResponse;
use axum::extract::State;
use axum::response::Html;
use axum::Form;
use axum_login::AuthSession;
use axum_template::RenderHtml;
use git_version::git_version;
use serde::Deserialize;
use serde_json::json;
use sqlx::Pool;

#[derive(Deserialize)]
pub(crate) struct WorkerEditForm {
    worker: Option<i64>,
    creating: Option<bool>,
}

pub(crate) async fn workeredit(
    State(AppState { pool, engine, .. }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(worker): Form<WorkerEditForm>,
) -> Result<impl IntoResponse, CustomError> {
    let id = get_admin(&auth)?;

    let users = sqlx::query_as!(
        Worker,
        "
        select * from users where deactivated = false order by id asc;
        "
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let mut selectlist = users
        .iter()
        .map(|w| (w.id, w.name.as_str()))
        .collect::<Vec<_>>();

    let data = serde_json::json!({
    "git_ver": git_version!(),
        "admin": true,
        "logged_in": true,
        "title": "CZ4R Worker Edit",
        "target": "worker-edit",
        "creating": worker.creating == Some(true),
        "selected": worker.worker,
        "selectlist": selectlist,
        "own_id": id,
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

    Ok(RenderHtml("workeredit.hbs", engine, data))
}
