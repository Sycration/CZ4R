use crate::{errors::CustomError, AppEngine, AppState, Job, JobWorker};
use crate::{get_admin, Backend};
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect},
    Form,
};
use axum_login::AuthSession;
use axum_template::RenderHtml;
use git_version::git_version;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use sqlx::{query, query_as, Pool};

pub(crate) async fn admin(
    State(AppState {
        pool: _, engine, ..
    }): State<AppState>,
    mut auth: AuthSession<Backend>,
) -> Result<impl IntoResponse, CustomError> {
    get_admin(&auth)?;

    let data = serde_json::json!({
    "git_ver": git_version!(),
        "admin": true,
        "logged_in": true,
        "title": "CZ4R"
    });

    Ok(RenderHtml("admin.hbs", engine, data))
}
