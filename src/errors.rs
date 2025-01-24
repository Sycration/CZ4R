use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use axum_template::{engine::Engine, RenderHtml, TemplateEngine};
use handlebars::Handlebars;
use std::{borrow::Borrow, fmt};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::{render, setup_handlebars};

#[derive(Debug)]
pub struct CustomError(pub anyhow::Error);

impl<E> From<E> for CustomError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

impl IntoResponse for CustomError {
    fn into_response(self) -> Response {
        let mut hbs = Handlebars::new();
        hbs.set_strict_mode(true);
        setup_handlebars(&mut hbs);

        let data = serde_json::json!({
            "admin": false,
            "logged_in": false,
            "title": "CZ4R Error 404",
            "cause": self.0.to_string() ,
        });
        info!("Client error\n{}\n{}", self.0, self.0.backtrace());

        let html = hbs.render("errorauth.hbs", &data);

        let mut html = if let Ok(html) = html {
            html
        } else {
            format!("ERROR\n\n{}", self.0)
        };

        let mut res = Html(html).into_response();
        *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        res
    }
}
