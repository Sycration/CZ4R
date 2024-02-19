use crate::Backend;
use crate::{errors::CustomError, AppEngine, AppState, Job, JobWorker};
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect},
    Form,
};
use axum_login::AuthSession;
use axum_template::RenderHtml;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use sqlx::{query, query_as, Pool, Postgres};

pub(crate) async fn index(
    State(engine): State<AppEngine>,
    mut auth: AuthSession<Backend>,
) -> Result<impl IntoResponse, CustomError> {
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.user.is_some();

    let data = serde_json::json!({
        "admin": admin,
        "logged_in": logged_in,
        "title": "CZ4R"
    });

    Ok(RenderHtml("home.hbs", engine, data))
}
