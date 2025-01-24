use std::convert::Infallible;

use crate::r#static::{static_handler, StaticAsset};
use crate::Backend;
use crate::{errors::CustomError, AppEngine, AppState, Job, JobWorker};
use axum::http::Uri;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect},
    Form,
};
use axum_login::AuthSession;
use axum_template::RenderHtml;
use itertools::Itertools;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use sqlx::{query, query_as, Pool};
use tracing::debug;
pub(crate) async fn error404(
    uri: Uri,
    State(AppState {
        pool: _, engine, ..
    }): State<AppState>,
    mut auth: AuthSession<Backend>,
) -> Result<impl IntoResponse, Infallible> {
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.user.is_some();

    if StaticAsset::iter().contains(uri.path().strip_prefix('/').unwrap_or(uri.path())) {
        return Ok(static_handler(uri).await.into_response());
    }

    let data = serde_json::json!({
        "admin": admin,
        "logged_in": logged_in,
        "title": "CZ4R Error 404"
    });

    debug!("error 404 at url {}", uri);

    Ok(RenderHtml("error404.hbs", engine, data).into_response())
}
