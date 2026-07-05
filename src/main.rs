#![allow(unused_mut)]
#![allow(unused_imports)]
#![allow(non_snake_case)]

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use axum::{
    BoxError, Form, Router, body::Body, debug_handler, error_handling::HandleErrorLayer, extract::{Extension, FromRef, Path, Request, State}, http::{Response, StatusCode, Uri}, middleware::{self, Next}, response::{Html, IntoResponse, Redirect}, routing::{get, post, put},
};
use axum_login::tower_sessions::ExpiredDeletion;
use axum_login::{
    AuthManagerLayerBuilder, AuthUser, AuthnBackend, UserId, tower_sessions::SessionManagerLayer,
};
use axum_login::{AuthSession, tower_sessions::Expiry};
use axum_template::{Key, RenderHtml, engine::Engine};
use config::Config;
use errors::CustomError;
use futures::join;
use handlebars::{Handlebars, handlebars_helper};
use login::{LoginForm, loginpage};
use rust_embed::RustEmbed;
use scrypt::{
    Scrypt,
    password_hash::{
        self, PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
    },
};
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;
use shutdown::shutdown_signal;
use sqlx::{Pool, Sqlite, query, query_as};
use sqlx::{migrate::MigrateDatabase, types::time::Date};
use r#static::static_handler;
use std::time::Instant;
use std::{
    collections::{BTreeMap, HashMap},
    default, env, fmt,
    net::SocketAddr,
    str::FromStr,
    sync::{Arc, OnceLock},
};
use std::{fs::File, future::IntoFuture};
use time::{OffsetDateTime, Time, UtcOffset};
use tokio::runtime::Builder;
use tokio::sync::RwLock;
use tower::ServiceBuilder;
use tower_http::trace::{self, TraceLayer};
use tower_sessions_sqlx_store::{SqliteStore, sqlx::SqlitePool};
use tracing::Level;
use tracing::{debug, info, trace, warn};
use tracing_subscriber::{EnvFilter, Layer, filter};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa_swagger_ui::SwaggerUi;

mod admin;
mod api_auth;
mod change_pw;
mod change_worker;
mod checkinout;
mod config;
mod create_worker;
mod deactivate;
mod error404;
mod errors;
mod export_db;
mod index;
mod jobedit;
mod joblist;
mod login;
mod reset_pw;
mod restore;
mod shutdown;
mod r#static;
mod whoami;
mod workerdata;
mod workeredit;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Job {
    id: i64,
    sitename: String,
    workorder: String,
    servicecode: String,
    address: String,
    date: Date,
    notes: String,
}

#[derive(Debug, Default, Clone, sqlx::FromRow, Serialize)]
pub struct Worker {
    id: i64,
    name: String,
    hash: String,
    salt: String,
    admin: bool,
    address: String,
    phone: String,
    email: String,
    rate_hourly_cents: i64,
    rate_mileage_cents: i64,
    rate_drive_hourly_cents: i64,
    flat_rate_cents: i64,
    must_change_pw: bool,
    deactivated: bool,
    logged_out: bool,
}

#[derive(Debug, Default, Clone, sqlx::FromRow)]

pub struct JobWorker {
    job: i64,
    worker: i64,
    signin: Option<Time>,
    signout: Option<Time>,
    miles_driven: f32,
    hours_driven: f32,
    extraexpcents: i64,
    notes: String,
    using_flat_rate: bool,
}

type AppEngine = Engine<Handlebars<'static>>;

#[derive(Clone, FromRef)]
pub struct AppState {
    pool: Pool<Sqlite>,
    engine: AppEngine,
    db_url: String,
}

impl AuthUser for Worker {
    type Id = i64;
    fn id(&self) -> i64 {
        self.id
    }

    fn session_auth_hash(&self) -> &[u8] {
        self.hash.as_bytes()
    }
}

#[derive(Debug, Clone)]
pub struct Backend {
    db: Pool<Sqlite>,
}

impl Backend {
    fn new(db: Pool<Sqlite>) -> Self {
        Self { db }
    }
}

impl AuthnBackend for Backend {
    type User = Worker;
    type Credentials = LoginForm;
    type Error = sqlx::Error;
    async fn authenticate(
        &self,
        creds: Self::Credentials,
    ) -> Result<Option<Self::User>, Self::Error> {
        let user = query_as!(
            Worker,
            "select * from users where name = $1",
            creds.username
        )
        .fetch_optional(&self.db)
        .await?;

        let filtered = user.filter(|user| {
            let salt = &user.salt;
            let saltstr: Result<SaltString, password_hash::Error> =
                SaltString::from_b64(salt.as_str());
            let saltstr = if let Ok(s) = saltstr {
                s
            } else {
                return false;
            };

            let challenge_hash = Scrypt
                .hash_password(creds.password.as_bytes(), saltstr.as_salt())
                .unwrap()
                .to_string();

            let res = challenge_hash == user.hash;

            if res {
                debug!(
                    "user {} (id {}) successfully authenticated",
                    user.name, user.id
                );
            } else {
                debug!("user {} (id {}) failed to authenticate", user.name, user.id);
            }

            res
        });

        if filtered.is_none() {
            debug!(
                "nonexistent user {} attempted to authenticate",
                creds.username
            );
        }

        return Ok(filtered);
    }

    async fn get_user(&self, user_id: &UserId<Self>) -> Result<Option<Self::User>, Self::Error> {
        let user = query_as!(Worker, "select * from users where id = $1", user_id)
            .fetch_optional(&self.db)
            .await?;

        match &user {
            Some(u) => {
                debug!("found user {} with id {}", u.name, user_id);

                if u.logged_out {
                    debug!("user {} (id {}) is logged out", u.name, user_id);
                    return Ok(None);
                }
            }
            None => {
                debug!("could not find user with id {}", user_id);
            }
        }

        Ok(user)
    }
}

pub static TZ_OFFSET: OnceLock<UtcOffset> = OnceLock::new();

fn main() {
    let _ = dotenvy::dotenv();

    let stdout_log = tracing_subscriber::fmt::layer().pretty();

    tracing_subscriber::registry()
        .with(stdout_log.with_filter(filter::EnvFilter::from_default_env()))
        .init();

    debug!("logging initialized");

    let tz_offset = TZ_OFFSET.get_or_init(|| OffsetDateTime::now_local().unwrap().offset());
    info!("The timezone offset is {tz_offset}");

    let rt = Builder::new_multi_thread().enable_all().build().unwrap();

    rt.block_on(app());
}
#[cfg(debug_assertions)]
pub fn setup_handlebars(hbs: &mut Handlebars) {
    use handlebars::DirectorySourceOptions;
    let mut dso = DirectorySourceOptions::default();
    dso.tpl_extension = "".to_string();

    hbs.set_dev_mode(true);
    hbs.register_templates_directory("./hb-templates", dso)
        .unwrap();
    debug!("setup handlebars");
}

#[cfg(not(debug_assertions))]
#[derive(RustEmbed)]
#[folder = "hb-templates"]
struct Templates;

#[cfg(not(debug_assertions))]

pub fn setup_handlebars(hbs: &mut Handlebars) {
    hbs.set_dev_mode(false);
    hbs.register_embed_templates::<Templates>().unwrap();
    debug!("setup handlebars");
}
pub const USER_TAG: &str = "user";
pub const ADMIN_TAG: &str = "admin";

/// Registers the `bearer_auth` security scheme (`Authorization: Bearer
/// <token>`, as issued by `POST /api/v1/login`) with the generated OpenAPI
/// spec, so Swagger UI shows an "Authorize" button and every endpoint
/// annotated with `security(("bearer_auth" = []))` documents that it needs
/// one.
struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};

        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer_auth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("opaque")
                    .description(Some(
                        "The token returned by `POST /api/v1/login`. Send as \
                         `Authorization: Bearer <token>`.",
                    ))
                    .build(),
            ),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "CZ4R API",
        license(name = "AGPL-3.0", url = "https://www.gnu.org/licenses/agpl-3.0.en.html"),
    ),
    tags(
        (name = USER_TAG, description = "User API endpoints"),
        (name = ADMIN_TAG, description = "Admin API endpoints")
    ),
    modifiers(&SecurityAddon)
)]
struct ApiDoc;

async fn app() {
    let mut hbs = Handlebars::new();
    hbs.set_strict_mode(true);
    setup_handlebars(&mut hbs);
    handlebars_helper!(eq: |a: Value, b: Value| a == b);
    handlebars_helper!(neq: |a: Value, b: Value| a != b);
    hbs.register_helper("eq", Box::new(eq));
    hbs.register_helper("neq", Box::new(neq));

    let config = config::Config::new().await;

    let app_pool: Pool<Sqlite> = config.create_pool().await;
    let auth_pool = config.create_pool().await;
    let backend_pool = config.create_pool().await;
    let session_pool = config.create_pool().await;

    let Config {
        database_url,
        login_secret: _,
        port,
        site_url: _,
        backup_task,
        session_ttl,
        session_check_time,
    } = config;

    let backend = Backend::new(backend_pool);

    let session_store = SqliteStore::new(auth_pool);
    session_store.migrate().await.unwrap();

    let deletion_task = tokio::task::spawn(
        session_store
            .clone()
            .continuously_delete_expired(tokio::time::Duration::from_secs(session_check_time)),
    );

    let session_layer = SessionManagerLayer::new(session_store)
        .with_expiry(Expiry::OnInactivity(time::Duration::seconds(session_ttl)))
        .with_always_save(true);

    let auth_layer = AuthManagerLayerBuilder::new(backend, session_layer.clone()).build();

    async fn log_url_middleware(request: Request<Body>, next: Next) -> Response<Body> {
        // Log the incoming URI
        debug!("Incoming Request URL: {}", request.uri());

        // Pass the request to the next handler/middleware
        next.run(request).await
    }

    // Every action below is split into two handlers that share the same
    // "core" business-logic function (see each module):
    //   - a `/web/v1/...` handler that takes an HTML form and returns HTML
    //     (a redirect, or a status code, for the htmx-driven UI), and
    //   - an `/api/v1/...` handler that is a proper REST/JSON endpoint,
    //     taking and returning JSON, documented with `#[utoipa::path]` and
    //     collected into the OpenAPI spec via `routes!`.
    let admin_only = OpenApiRouter::new()
        .route("/admin", get(admin::admin))
        .route("/admin/worker-edit", get(workeredit::workeredit))
        .route("/admin/worker-data", get(workerdata::workerdatapage))
        .route("/admin/restore", get(restore::restorepage))
        .route("/admin/jobedit", get(jobedit::jobeditpage))
        // web (HTML forms, htmx)
        .route(
            "/admin/web/v1/create-worker",
            post(create_worker::create_worker),
        )
        .route("/admin/web/v1/edit-job", post(jobedit::jobedit))
        .route("/admin/web/v1/delete-job", post(jobedit::jobdelete))
        .route("/admin/web/v1/logout-worker", post(login::logout_user))
        .route(
            "/admin/web/v1/deactivate-worker",
            post(deactivate::deactivate),
        )
        .route(
            "/admin/web/v1/change-worker",
            post(change_worker::change_worker),
        )
        .route("/admin/web/v1/restore-worker", post(restore::restore))
        .route("/admin/web/v1/reset-pw", post(reset_pw::reset_pw))
        // api (JSON REST)
        .routes(routes!(export_db::export_db))
        .routes(routes!(create_worker::create_worker_api))
        .routes(routes!(jobedit::create_job_api))
        .routes(routes!(
            jobedit::jobeditpage_api,
            jobedit::update_job_api,
            jobedit::delete_job_api,
        ))
        .routes(routes!(workeredit::list_users_api))
        .routes(routes!(workeredit::get_user_api))
        .routes(routes!(login::logout_user_api))
        .routes(routes!(deactivate::deactivate_api))
        .routes(routes!(change_worker::change_worker_api))
        .routes(routes!(restore::restore_api))
        .routes(routes!(reset_pw::reset_pw_api));

    let (app, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .route("/", get(index::index))
        .route("/joblist", get(joblist::joblistpage))
        .route("/loginpage", get(loginpage))
        .route("/checkinout", get(checkinout::checkinoutpage))
        .route("/change-pw", get(change_pw::change_pw_page))
        // web (HTML forms, htmx)
        .route("/web/v1/login", post(login::login))
        .route("/web/v1/logout", post(login::logout))
        .route("/web/v1/change-pw", post(change_pw::change_pw))
        .route("/web/v1/checkinout", post(checkinout::checkinout))
        // api (JSON REST)
        .routes(routes!(login::login_api))
        .routes(routes!(login::logout_api))
        .routes(routes!(login::refresh_token_api))
        .routes(routes!(change_pw::change_pw_api))
        .routes(routes!(checkinout::checkinout_api))
        .routes(routes!(checkinout::assignment_data_api))
        .routes(routes!(joblist::joblist_api))
        .routes(routes!(index::index_api))
        .routes(routes!(whoami::whoami))
        .merge(admin_only)
        .fallback(error404::error404)
        .layer(auth_layer)
        //.layer(session_layer)
        .with_state(AppState {
            pool: app_pool,
            engine: Engine::from(hbs),
            db_url: database_url,
        })
        .layer(middleware::from_fn(log_url_middleware))
        .split_for_parts();

    let app = app.merge(SwaggerUi::new("/swagger-ui").url("/openapi.json", api));

    // run it

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    info!("listening on {}", addr);

    let backup_handle = backup_task.as_ref().map(|t| t.abort_handle());
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(deletion_task.abort_handle(), backup_handle))
        .into_future();
    if let Some(backup_task) = backup_task {
        let (_, _, _) = join!(server, backup_task, deletion_task);
    } else {
        let (_, _) = join!(server, deletion_task);
    }
}

pub fn render<F>(f: F) -> Html<String>
where
    F: FnOnce(&mut Vec<u8>) -> Result<(), std::io::Error>,
{
    let mut buf = Vec::new();
    f(&mut buf).expect("Error rendering template");
    let html: String = String::from_utf8_lossy(&buf).into();
    Html(html)
}

pub fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc().to_offset(*TZ_OFFSET.get().unwrap())
}

pub fn empty_string_as_none<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: fmt::Display,
{
    let opt = Option::<String>::deserialize(de)?;
    match opt.as_deref().map(|s| s.trim()) {
        None | Some("") => Ok(None),

        Some(s) => FromStr::from_str(s).map_err(de::Error::custom).map(Some),
    }
}

/// The authenticated caller of a request, regardless of *how* they
/// authenticated. The HTML/htmx UI authenticates via a cookie-backed
/// [`AuthSession<Backend>`] (see [`current_user`]); the JSON REST API
/// authenticates via an `Authorization: Bearer <token>` header (see
/// [`crate::api_auth::ApiAuth`]). Business logic (the `*_core` functions in
/// every module) only ever deals with this type, so it doesn't need to care
/// which transport was used.
#[derive(Debug, Clone, Serialize)]
pub struct CurrentUser {
    pub id: i64,
    pub name: String,
    pub admin: bool,
}

/// Build a [`CurrentUser`] from a cookie session, for the HTML/htmx-facing
/// handlers.
pub fn current_user(auth: &AuthSession<Backend>) -> Option<CurrentUser> {
    auth.user.as_ref().map(|u| CurrentUser {
        id: u.id,
        name: u.name.clone(),
        admin: u.admin,
    })
}

pub fn get_user(user: Option<&CurrentUser>) -> Result<(i64, String, bool), CustomError> {
    if let Some(u) = user {
        Ok((u.id, u.name.clone(), u.admin))
    } else {
        Err(CustomError::new(
            anyhow!("Not logged in"),
            StatusCode::UNAUTHORIZED,
        ))
    }
}

pub fn get_admin(user: Option<&CurrentUser>) -> Result<(i64, String), CustomError> {
    let (id, name, admin) = get_user(user)?;
    if admin {
        Ok((id, name))
    } else {
        Err(CustomError::new(
            anyhow!("User {} does not have administrator privileges", id),
            StatusCode::FORBIDDEN,
        ))
    }
}
