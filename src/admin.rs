use crate::{errors::CustomError,  Job, JobWorker, AppState, AppEngine};
use axum::{
    extract::{Path, State},
    response::{Html, Redirect, IntoResponse},
    Form,
};
use crate::Backend;
use axum_login::AuthSession;
use axum_template::RenderHtml;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use sqlx::{query, query_as, Pool, Postgres};

pub(crate) async fn admin(
    State(AppState { pool: _, engine }): State<AppState>,
    mut auth: AuthSession<Backend>,
) -> Result<impl IntoResponse, CustomError> {
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.user.is_some();

    let data = serde_json::json!({
        "admin": admin,
        "logged_in": logged_in,
        "title": "CZ4R"
    });

    Ok(RenderHtml("admin.hbs",engine,data))
}
