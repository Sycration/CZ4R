#![allow(unused_mut)]
#![allow(unused_imports)]
#![allow(non_snake_case)]

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use axum::{
    debug_handler,
    error_handling::HandleErrorLayer,
    extract::{Extension, FromRef, Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
    routing::{get, post, put},
    BoxError, Form, Router,
};
use axum_login::tower_sessions::ExpiredDeletion;
use axum_login::{tower_sessions::Expiry, AuthSession};
use axum_login::{
    tower_sessions::{MemoryStore, SessionManagerLayer},
    AuthManagerLayerBuilder, AuthUser, AuthnBackend, UserId,
};
use axum_template::{engine::Engine, Key, RenderHtml};
use config::Config;
use errors::CustomError;
use futures::join;
use handlebars::{handlebars_helper, Handlebars};
use login::{loginpage, LoginForm};
use password_hash::{PasswordHasher, Salt, SaltString};
use r#static::static_handler;
use rand::{thread_rng, Rng};
use rust_embed::RustEmbed;
use scrypt::Scrypt;
use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::Value;
use shutdown::shutdown_signal;
use sqlx::{migrate::MigrateDatabase, types::time::Date};
use sqlx::{query, query_as, Pool, Sqlite};
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
use tower_sessions_sqlx_store::{sqlx::SqlitePool, SqliteStore};
use tracing::Level;
use tracing::{debug, info, trace, warn};
use tracing_subscriber::{filter, EnvFilter, Layer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod admin;
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

#[async_trait]
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
            .continuously_delete_expired(tokio::time::Duration::from_secs(session_ttl)),
    );

    let session_layer = SessionManagerLayer::new(session_store)
        .with_expiry(Expiry::OnInactivity(time::Duration::seconds(
            session_check_time,
        )))
        .with_always_save(true);

    let auth_layer = AuthManagerLayerBuilder::new(backend, session_layer.clone()).build();

    let admin_only = Router::new()
        .route("/admin", get(admin::admin))
        .route("/admin/worker-edit", get(workeredit::workeredit))
        .route("/admin/worker-data", get(workerdata::workerdatapage))
        .route("/admin/restore", get(restore::restorepage))
        .route(
            "/admin/api/v1/create-worker",
            post(create_worker::create_worker),
        )
        .route("/admin/api/v1/edit-job", post(jobedit::jobedit))
        .route("/admin/api/v1/delete-job", post(jobedit::jobdelete))
        .route(
            "/admin/api/v1/deactivate-worker",
            post(deactivate::deactivate),
        )
        .route(
            "/admin/api/v1/change-worker",
            post(change_worker::change_worker),
        )
        .route("/admin/api/v1/restore-worker", post(restore::restore))
        .route(
            "/admin/api/v1/export-database.sql",
            get(export_db::export_db),
        )
        .route("/admin/api/v1/reset-pw", post(reset_pw::reset_pw));

    let app = Router::new()
        .route("/", get(index::index))
        .route("/joblist", get(joblist::joblistpage))
        .route("/jobedit", get(jobedit::jobeditpage))
        .route("/loginpage", get(loginpage))
        .route("/login", post(login::login))
        .route("/logout", post(login::logout))
        .route("/checkinout", get(checkinout::checkinoutpage))
        .route("/change-pw", get(change_pw::change_pw_page))
        .route("/api/v1/change-pw", post(change_pw::change_pw))
        .route("/api/v1/checkinout", post(checkinout::checkinout))
        .merge(admin_only)
        .fallback(error404::error404)
        .layer(auth_layer)
        .layer(session_layer)
        .with_state(AppState {
            pool: app_pool,
            engine: Engine::from(hbs),
            db_url: database_url,
        });

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

pub fn get_user(auth: AuthSession<Backend>) -> Result<(i64, String, bool), CustomError> {
    if let Some((id, name, admin)) = auth.user.map(|u| (u.id, u.name, u.admin)) {
        Ok((id, name, admin))
    } else {
        Err(CustomError(anyhow!("Not logged in")))
    }
}

pub fn get_admin(auth: AuthSession<Backend>) -> Result<(i64, String), CustomError> {
    let (id, name, admin) = get_user(auth)?;
    if admin {
        Ok((id, name))
    } else {
        Err(CustomError(anyhow!(
            "User {} does not have administrator privileges",
            id
        )))
    }
}
