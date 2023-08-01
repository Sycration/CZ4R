use crate::{errors::CustomError, Auth, Job, JobWorker, AppState, AppEngine};
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

pub(crate) async fn error404(
    State(AppState { pool, engine }): State<AppState>,
    mut auth: Auth,
) -> Result<impl IntoResponse, CustomError>  {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = auth.current_user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.current_user.is_some();

    let data = serde_json::json!({
        "admin": admin,
        "logged_in": logged_in,
        "title": "CZ4R Error 404"
    });

    Ok(RenderHtml("error404.hbs",engine,data))

}