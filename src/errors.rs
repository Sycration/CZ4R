use crate::{render, setup_handlebars};
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Json, Response},
};
use axum_template::{engine::Engine, RenderHtml, TemplateEngine};
use git_version::git_version;
use handlebars::Handlebars;
use serde::Serialize;
use std::{borrow::Borrow, fmt};
use thiserror::Error;
use tracing::{debug, info, warn};

/// The single shared error type for all "business logic" / "core" functions
/// in this crate.
///
/// It carries both the underlying `anyhow::Error` (for logging/diagnostics)
/// and an HTTP status code describing how severe the failure is. Because it
/// is completely renderer-agnostic, the exact same `CustomError` produced by
/// a core function can be turned into either an HTML error page (via its own
/// `IntoResponse` impl, used by "web" handlers) or a JSON error body (via
/// `ApiError`, used by "api" handlers).
#[derive(Debug)]
pub struct CustomError {
    pub error: anyhow::Error,
    pub status: StatusCode,
}

impl CustomError {
    /// Build a `CustomError` with an explicit status code.
    pub fn new(error: impl Into<anyhow::Error>, status: StatusCode) -> Self {
        Self {
            error: error.into(),
            status,
        }
    }
}

impl<E> From<E> for CustomError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self {
            error: err.into(),
            status: StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl fmt::Display for CustomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl IntoResponse for CustomError {
    fn into_response(self) -> Response {
        let mut hbs = Handlebars::new();
        hbs.set_strict_mode(true);
        setup_handlebars(&mut hbs);

        let data = serde_json::json!({
        "git_ver": git_version!(),
            "admin": false,
            "logged_in": false,
            "title": "CZ4R Error 404",
            "cause": self.error.to_string() ,
        });
        info!("Client error\n{}\n{}", self.error, self.error.backtrace());

        let html = hbs.render("errorauth.hbs", &data);

        let html = if let Ok(html) = html {
            html
        } else {
            format!("ERROR\n\n{}", self.error)
        };

        let mut res = Html(html).into_response();
        *res.status_mut() = self.status;
        res
    }
}

/// The JSON counterpart to [`CustomError`].
///
/// "Api" handlers work with `Result<T, ApiError>` instead of
/// `Result<T, CustomError>`. This type wraps a `CustomError` and renders it
/// as a `{"error": "..."}` JSON body with the same status code, instead of
/// an HTML page.
#[derive(Debug)]
pub struct ApiError(pub CustomError);

impl From<CustomError> for ApiError {
    fn from(err: CustomError) -> Self {
        Self(err)
    }
}

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(CustomError::from(err))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        warn!(
            "API client error\n{}\n{}",
            self.0.error,
            self.0.error.backtrace()
        );

        let body = Json(serde_json::json!({
            "error": self.0.error.to_string(),
        }));

        (self.0.status, body).into_response()
    }
}

/// Turn the result of a "core" business-logic function into the response an
/// "api" handler should return: a JSON body on success, or a JSON error
/// object (with the appropriate status code) on failure.
///
/// This is the wrapper that lets `*_api` handlers stay a one-liner: extract
/// the JSON input, hand it to the shared `*_core` function, and pass the
/// result through `to_api`.
pub fn to_api<T>(result: Result<T, CustomError>) -> Result<Json<T>, ApiError>
where
    T: Serialize,
{
    result.map(Json).map_err(ApiError::from)
}

/// Same as [`to_api`], but wraps the success value with an explicit status
/// code (e.g. `StatusCode::CREATED` for a resource-creation endpoint).
pub fn to_api_with_status<T>(
    result: Result<T, CustomError>,
    status: StatusCode,
) -> Result<(StatusCode, Json<T>), ApiError>
where
    T: Serialize,
{
    result.map(|v| (status, Json(v))).map_err(ApiError::from)
}
