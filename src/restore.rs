use crate::errors::CustomError;
use crate::AppState;

use super::Worker;

use crate::Backend;
use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::Form;
use axum_login::AuthSession;
use axum_template::RenderHtml;
use password_hash::SaltString;
use scrypt::password_hash::PasswordHasher;
use scrypt::Scrypt;
use serde::Deserialize;
use serde::Serialize;
use sqlx::query;
use sqlx::query_as;
use sqlx::Pool;
use sqlx::Postgres;

#[derive(Deserialize)]
pub struct RestoreForm {
    user: i64,
}

#[derive(Serialize, Deserialize)]
struct RestoreListItem {
    id: i64,
    name: String,
}

pub async fn restorepage(
    State(AppState { pool, engine }): State<AppState>,
    mut auth: AuthSession<Backend>,
) -> Result<impl IntoResponse, CustomError> {
    let logged_in = auth.user.is_some();
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);

    if !admin {
        return Err(CustomError::Auth("Not logged in as admin".to_string()));
    }

    let mut _conn = pool.acquire().await.unwrap();

    let workers = match query_as!(
        RestoreListItem,
        "select id, name from users where users.deactivated = true order by id asc"
    )
    .fetch_all(&pool)
    .await
    {
        Ok(w) => w,
        Err(e) => return Err(CustomError::Database(e.to_string())),
    };

    let data = serde_json::json!({
        "title": "CZ4R Restore Workers",
        "admin": admin,
        "logged_in": logged_in,
        "workers": workers
    });

    Ok(RenderHtml("restore.hbs", engine, data))
}

pub(crate) async fn restore(
    mut auth: AuthSession<Backend>,
    State(pool): State<Pool<Postgres>>,
    Form(restore_form): Form<RestoreForm>, //Extension(worker): Extension<Worker>
) -> Result<impl IntoResponse, CustomError> {
    if let Some(true) = auth.user.map(|u| u.admin) {
        let mut _conn = pool.acquire().await.unwrap();

        match query!(
            "update users set deactivated = false where id = $1",
            restore_form.user
        )
        .execute(&pool)
        .await
        {
            Ok(_) => {}
            Err(e) => return Err(CustomError::Database(e.to_string())),
        }

        Ok(Redirect::to("/admin/restore"))
    } else {
        Err(CustomError::Auth("Not logged in as admin".to_string()))
    }
}
