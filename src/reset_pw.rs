//Name=&Address=&Phone=&Email=&Hourly=&Mileage=&Drivetime=

use crate::errors::CustomError;
use axum::extract::Path;
use axum::extract::State;
use axum::response::Html;
use axum::response::Redirect;
use axum::Form;
use password_hash::PasswordHasher;
use password_hash::SaltString;
use rand::thread_rng;
use scrypt::Scrypt;
use serde::Deserialize;
use sqlx::query;
use sqlx::Pool;
use sqlx::Postgres;
use crate::Backend;
use axum_login::AuthSession;
#[derive(Deserialize)]
pub(crate) struct ResetPwForm {
    id: i64,
}

pub(crate) async fn reset_pw(
    State(pool): State<Pool<Postgres>>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<ResetPwForm>,
) -> Result<Redirect, CustomError> {
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
            return Err(CustomError::Database(e.to_string()));
        }

        Ok(Redirect::to(
            format!("/admin/worker-edit?worker={}", form.id).as_str(),
        ))
    } else {
        Err(CustomError::Auth("Not logged in".to_string()))
    }
}
