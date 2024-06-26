use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use axum_template::{engine::Engine, RenderHtml, TemplateEngine};
use handlebars::Handlebars;
use tracing::warn;
use std::{borrow::Borrow, fmt};

use crate::{render, setup_handlebars};

#[derive(Debug)]
pub enum CustomError {
    ClientData(String),
    FaultySetup(String),
    Database(String),
    Auth(String),
    AdminReqd(String),
}

impl CustomError {
    pub fn build(&self, hbs: &axum_template::engine::Engine<Handlebars<'static>> ) -> Response {


        let (status, error_message) = match self {
            CustomError::Database(message) => (StatusCode::UNPROCESSABLE_ENTITY, message),
            CustomError::FaultySetup(message) => (StatusCode::UNPROCESSABLE_ENTITY, message),
            CustomError::Auth(message) => (StatusCode::UNAUTHORIZED, message),
            CustomError::ClientData(message) => (StatusCode::UNPROCESSABLE_ENTITY, message),
            CustomError::AdminReqd(message) => (StatusCode::FORBIDDEN, message),
        };

        warn!("{}\tmessage = {}", status, error_message);



        let data = serde_json::json!({
            "admin": false,
            "logged_in": false,
            "title": "CZ4R Error 404",
            "cause": error_message
        });



        let html = hbs.render("errorauth.hbs", data);

        let mut html = if let Ok(html) = html {
            html
        } else {
            format!("status = {}, message = {}", status, error_message)
        };

        let mut res = Html(html).into_response();
        *res.status_mut() = status;
         res

    }
}


impl fmt::Display for CustomError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            CustomError::FaultySetup(ref cause) => write!(f, "Setup Error: {}", cause),
            CustomError::Database(ref cause) => {
                write!(f, "Database Error: {}", cause)
            }
            CustomError::Auth(ref cause) => write!(f, "Authentication Error: {}", cause),
            CustomError::AdminReqd(ref cause) => write!(f, "Admin Authentication Error: {}", cause),
            CustomError::ClientData(ref cause) => write!(f, "Invalid Client Data: {}", cause),
        }
    }
}

impl From<axum::http::uri::InvalidUri> for CustomError {
    fn from(err: axum::http::uri::InvalidUri) -> CustomError {
        CustomError::FaultySetup(err.to_string())
    }
}
