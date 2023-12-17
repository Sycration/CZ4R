use crate::{errors::CustomError, Job, JobWorker, AppState, AppEngine};
use axum::{
    extract::{Path, State},
    response::{Html, Redirect, IntoResponse},
    Form,
};
use axum_template::RenderHtml;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use sqlx::{query, query_as, Pool, Postgres};
use crate::Backend;
use axum_login::AuthSession;
pub(crate) async fn error404(
    State(AppState { pool, engine }): State<AppState>,
    mut auth: AuthSession<Backend>,
) -> Result<impl IntoResponse, CustomError>  {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.user.is_some();

    let data = serde_json::json!({
        "admin": admin,
        "logged_in": logged_in,
        "title": "CZ4R Error 404"
    });

    Ok(RenderHtml("error404.hbs",engine,data))

}