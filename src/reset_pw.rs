//Name=&Address=&Phone=&Email=&Hourly=&Mileage=&Drivetime=

use crate::errors::CustomError;
use crate::AppState;
use crate::Backend;
use axum::extract::Path;
use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::Form;
use axum_login::AuthSession;
use password_hash::PasswordHasher;
use password_hash::SaltString;
use rand::thread_rng;
use scrypt::Scrypt;
use serde::Deserialize;
use sqlx::query;
use sqlx::Pool;
use sqlx::Postgres;
#[derive(Deserialize)]
pub(crate) struct ResetPwForm {
    id: i64,
}


pub(crate) async fn reset_pw(
State(AppState { pool, engine }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<ResetPwForm>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    if auth.user.map(|w| w.admin) == Some(true) {
        let res = query!(
            r#"
        update users
        set must_change_pw = true
        where id = $1
        "#,
            form.id
        )
        .execute(&pool)
        .await;

        if let Err(e) = res {
            return Err(CustomError::Database(e.to_string()).build(&engine));
        }

        Ok(Redirect::to(
            format!("/admin/worker-edit?worker={}", form.id).as_str(),
        ))
    } else {
        Err(CustomError::Auth("Not logged in".to_string()).build(&engine))
    }
}
