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
use tracing::info;

pub(crate) async fn export_db(
    mut auth: AuthSession<Backend>,
    State(AppState { pool, engine, db_url }): State<AppState>,
) -> Result<impl IntoResponse, CustomError> {
    let (my_id, my_name) = get_admin(auth)?;

    let url = url::Url::parse(&db_url)?;
    let path = url.path();

    let res = Ok(
    Command::new("sqlite3")
    .arg(path)
    .arg(".dump")
    .output()
    .await?.stdout
    );

    info!("admin {} (id {}) exported the database", my_name, my_id);

    res

}
