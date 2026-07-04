use crate::errors::CustomError;
use crate::{current_user, get_admin};
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

/// `GET /admin/api/v1/export-database.sql` — not JSON (it streams a raw
/// `sqlite3 .dump` of the database), but still documented in the OpenAPI
/// spec for completeness.
#[utoipa::path(
    get,
    path = "/admin/api/v1/export-database.sql",
    responses((status = OK, description = "A raw `sqlite3 .dump` of the database.")),
    tag = super::ADMIN_TAG
)]
pub(crate) async fn export_db(
    auth: AuthSession<Backend>,
    State(AppState {
        pool: _,
        engine: _,
        db_url,
    }): State<AppState>,
) -> Result<impl IntoResponse, CustomError> {
    let (my_id, my_name) = get_admin(current_user(&auth).as_ref())?;

    let url = url::Url::parse(&db_url)?;
    let mut path = url.path().to_string();

    if let Some(domain) = url.domain() {
        if domain == "." {
            path.insert(0, '.');
        }
    }

    let res = Ok(Command::new("sqlite3")
        .arg(path)
        .arg(".dump")
        .output()
        .await?
        .stdout);

    info!("admin {} (id {}) exported the database", my_name, my_id);

    res
}
