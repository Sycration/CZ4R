use axum::{
    debug_handler,
    extract::{Extension, Path, State},
    response::{Html, Redirect},
    routing::get,
    Form, Router,
};
use axum_login::{
    axum_sessions::{async_session::MemoryStore as SessionMemoryStore, SessionLayer},
    extractors::AuthContext,
    memory_store::MemoryStore as AuthMemoryStore,
    secrecy::SecretVec,
    AuthLayer, AuthUser, PostgresStore, RequireAuthorizationLayer,
};
use errors::CustomError;
use password_hash::{PasswordHasher, Salt, SaltString};
use rand::{thread_rng, Rng};
use scrypt::Scrypt;
use serde::Deserialize;
use sqlx::types::time::Date;
use sqlx::{query, query_as, Pool, Postgres};
use std::{
    collections::HashMap,
    default, env,
    net::SocketAddr,
    sync::{Arc, OnceLock},
};
use time::{OffsetDateTime, Time, UtcOffset};
use tokio::runtime::Builder;
use tokio::sync::RwLock;
use tower_http::trace::{self, TraceLayer};
use tracing::Level;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod change_pw;
mod change_worker;
mod checkinout;
mod config;
mod create_worker;
mod errors;
mod jobedit;
mod joblist;
mod login;
mod reset_pw;
mod workerdata;
mod workeredit;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Job {
    id: i64,
    sitename: String,
    workorder: String,
    servicecode: String,
    address: String,
    date: Date,
}

#[derive(Debug, Default, Clone, sqlx::FromRow)]

pub struct Worker {
    id: i64,
    name: String,
    hash: String,
    salt: String,
    admin: bool,
    address: String,
    phone: String,
    email: String,
    rate_hourly_cents: i32,
    rate_mileage_cents: i32,
    rate_drive_hourly_cents: i32,
    flat_rate_cents: i32,
    must_change_pw: bool,
}

#[derive(Debug, Default, Clone, sqlx::FromRow)]

pub struct JobWorker {
    job: i64,
    worker: i64,
    signin: Option<Time>,
    signout: Option<Time>,
    miles_driven: f32,
    hours_driven: f32,
    extraexpcents: i32,
    notes: String,
    using_flat_rate: bool,
}

type Auth = AuthContext<i64, Worker, PostgresStore<Worker, ()>, ()>;

impl AuthUser<i64> for Worker {
    fn get_id(&self) -> i64 {
        self.id
    }

    fn get_password_hash(&self) -> axum_login::secrecy::SecretVec<u8> {
        SecretVec::new(self.hash.clone().into())
    }
}

pub static TZ_OFFSET: OnceLock<UtcOffset> = OnceLock::new();

fn main() {
    eprintln!(
        "The timezone offset is {}",
        TZ_OFFSET.get_or_init(|| { OffsetDateTime::now_local().unwrap().offset() })
    );

    let rt = Builder::new_multi_thread().enable_all().build().unwrap();

    rt.block_on(app());
}

async fn app() {
    dbg!(now());

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().compact())
        .init();

    let config = config::Config::new();

    let pool = config.create_pool().await;

    let secret = config.login_secret.clone();

    let session_store = SessionMemoryStore::new();
    let session_layer = SessionLayer::new(session_store, &secret).with_secure(false);

    let user_store = PostgresStore::<Worker>::new(pool.clone());

    let auth_layer: AuthLayer<PostgresStore<Worker, ()>, i64, Worker, ()> =
        AuthLayer::new(user_store, &secret);

    //check if no users, create it from env vars otherwise
    let mut conn = pool.acquire().await.unwrap();
    let a = query!("select count(*) from users")
        .fetch_one(&mut conn)
        .await
        .unwrap()
        .count
        .unwrap();
    if a == 0 {
        let admin_uname = env::var("ADMIN_USER").expect("ADMIN_USER not set");
        let admin_pw = env::var("ADMIN_PASSWORD").expect("ADMIN_PASSWORD not set");

        let salt = SaltString::generate(&mut thread_rng());

        let hash = Scrypt
            .hash_password(admin_pw.as_bytes(), salt.as_salt())
            .unwrap()
            .to_string();

        query!(
            "insert into users (name, hash, salt, admin) values ($1, $2, $3, $4);",
            admin_uname,
            hash,
            salt.as_str(),
            true
        )
        .execute(&mut conn)
        .await
        .expect("Failed to insert default admin user");
    }

    // build our application with a route
    let app = Router::new()
        .route("/", get(index))
        .route("/joblist", get(joblist::joblistpage))
        .route("/jobedit", get(jobedit::jobeditpage))
        .route("/loginpage", get(loginpage))
        .route("/login", get(login::login))
        .route("/logout", get(login::logout))
        .route("/checkinout", get(checkinout::checkinoutpage))
        .route("/change-pw", get(change_pw::change_pw_page))
        .route("/api/v1/change-pw/:id", get(change_pw::change_pw))
        .route("/api/v1/checkinout", get(checkinout::checkinout))
        .route("/admin", get(admin))
        .route("/admin/worker-edit", get(workeredit::workeredit))
        //.route("/admin/worker-create", get(workercreate))
        .route("/admin/worker-data", get(workerdata::workerdatapage))
        .route("/admin/deactivated-workers", get(deactivatedworkers))
        .route(
            "/admin/api/v1/create-worker",
            get(create_worker::create_worker),
        )
        .route("/admin/api/v1/edit-job", get(jobedit::jobedit))
        .route(
            "/admin/api/v1/change-worker/:id",
            get(change_worker::change_worker),
        )
        .route("/admin/api/v1/reset-pw", get(reset_pw::reset_pw))
        .fallback(error404)
        .layer(Extension(config))
        .layer(auth_layer)
        .layer(session_layer)
        .with_state(pool);

    // run it
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn index(
    State(pool): State<Pool<Postgres>>,
    mut auth: Auth,
) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = auth.current_user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.current_user.is_some();

    Ok(crate::render(|buf| {
        crate::templates::index_html(buf, "CZ4R Home", admin, logged_in)
    }))
}

async fn deactivatedworkers(
    State(pool): State<Pool<Postgres>>,
    mut auth: Auth,
) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = auth.current_user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.current_user.is_some();

    Ok(crate::render(|buf| {
        crate::templates::deactivatedworkers_html(buf, "CZ4R Deleted Workers", admin)
    }))
}

async fn admin(
    State(pool): State<Pool<Postgres>>,
    mut auth: Auth,
) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = auth.current_user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.current_user.is_some();

    Ok(crate::render(|buf| {
        crate::templates::admin_html(buf, "CZ4R Admin Page", admin)
    }))
}

#[derive(Deserialize)]
pub struct LoginPageForm {
    failure: Option<bool>,
}

async fn loginpage(
    State(pool): State<Pool<Postgres>>,
    mut auth: Auth,
    Form(form): Form<LoginPageForm>,
) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    let logged_in = auth.current_user.is_some();

    Ok(crate::render(|buf| {
        crate::templates::login_html(buf, "CZ4R Login", logged_in, form.failure == Some(true))
    }))
}

async fn error404(
    State(pool): State<Pool<Postgres>>,
    mut auth: Auth,
) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = auth.current_user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.current_user.is_some();

    Ok(crate::render(|buf| {
        crate::templates::error404_html(buf, "CZ4R 404", admin)
    }))
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

// Include the generated source code
include!(concat!(env!("OUT_DIR"), "/templates.rs"));
