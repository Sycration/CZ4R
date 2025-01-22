use crate::errors::CustomError;
use crate::get_admin;
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
use serde::Deserialize;
use serde::Serialize;
use tokio::process;
use tokio::process::Command;

pub(crate) async fn export_db(
    mut auth: AuthSession<Backend>,
    State(AppState { pool, engine, db_url }): State<AppState>,
) -> Result<impl IntoResponse, CustomError> {
    get_admin(auth)?;

    Ok(
    Command::new("pg_dump")
    .arg("--inserts")
    .arg(db_url)
    .output()
    .await?.stdout

    )

    

}
