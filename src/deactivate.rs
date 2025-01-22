use crate::errors::CustomError;
use crate::get_admin;
use crate::AppState;

use super::Worker;

use anyhow::anyhow;
use anyhow::bail;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum_template::RenderHtml;
use sqlx::query;
use sqlx::query_as;

use crate::Backend;
use axum::extract::State;
use axum::Form;
use axum_login::AuthSession;
use password_hash::SaltString;
use scrypt::password_hash::PasswordHasher;
use scrypt::Scrypt;
use serde::Deserialize;
use serde::Serialize;
use sqlx::Pool;
use sqlx::Postgres;

#[derive(Deserialize)]
pub struct DeactivateForm {
    user: i64,
}

pub(crate) async fn deactivate(
    mut auth: AuthSession<Backend>,
    State(AppState { pool, engine, .. }): State<AppState>,
    Form(deactivate_form): Form<DeactivateForm>, //Extension(worker): Extension<Worker>
) -> Result<impl IntoResponse, impl IntoResponse> {
    let my_id = get_admin(auth)?;
    if deactivate_form.user == my_id {
        return Err(CustomError(anyhow!("A user cannot deactivate themselves")));
    }

    let mut _conn = pool.acquire().await.unwrap();

    query!(
        "update users set deactivated = true where id = $1;",
        deactivate_form.user
    )
    .execute(&pool)
    .await?;

    Ok(Redirect::to("/admin/worker-edit"))
}
