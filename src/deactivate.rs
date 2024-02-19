use crate::errors::CustomError;
use crate::AppState;

use super::Worker;

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
    State(pool): State<Pool<Postgres>>,
    Form(deactivate_form): Form<DeactivateForm>, //Extension(worker): Extension<Worker>
) -> Result<impl IntoResponse, CustomError> {
    if let Some((true, my_id)) = auth.user.map(|u| (u.admin, u.id)) {
        if deactivate_form.user == my_id {
            return Err(CustomError::Auth(
                "You cannot deactivate yourself".to_string(),
            ));
        }

        let mut _conn = pool.acquire().await.unwrap();

        match query!(
            "update users set deactivated = true where id = $1;",
            deactivate_form.user
        )
        .execute(&pool)
        .await
        {
            Ok(_) => {}
            Err(e) => return Err(CustomError::Database(e.to_string())),
        }

        Ok(Redirect::to("/admin/worker-edit"))
    } else {
        Err(CustomError::Auth("Not logged in as admin".to_string()))
    }
}
