mod config;
mod errors;

use crate::errors::CustomError;
use axum::{
    debug_handler,
    extract::{Extension, Path, State},
    response::Html,
    routing::get,
    Form, Router,
};
use axum_login::{memory_store::MemoryStore, axum_sessions::SessionLayer, SqlxStore, PostgresStore};
use rand::Rng;
use serde::Deserialize;
use sqlx::{Pool, Postgres};
use std::{net::SocketAddr, sync::Arc};

#[derive(Debug, Default, Clone, sqlx::FromRow)]
struct Worker {
    name: String,
    id: u64,
    admin: bool,
}



#[tokio::main]
async fn main() {
    let config = config::Config::new();

    let pool = config.create_pool().await;

    let mut secret = [0; 64];
    rand::thread_rng().fill(&mut secret);

    let session_store = MemoryStore::new();
    let session_layer = SessionLayer::new(session_store, &secret).with_secure(false);

    // build our application with a route
    let app = Router::new()
        .route("/", get(index))
        .route("/joblist", get(joblist))
        .route("/jobedit", get(jobedit))
        .route("/loginpage", get(loginpage))
        .route("/checkinout", get(checkinout))
        .route("/admin", get(admin))
        .route("/admin/worker-edit", get(workeredit))
        .route("/admin/worker-create", get(workercreate))
        .route("/admin/worker-data", get(workerdata))
        .route("/admin/deactivated-workers", get(deactivatedworkers))
        .fallback(error404)
        .layer(Extension(config))
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

async fn index(State(pool): State<Pool<Postgres>>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;

    Ok(crate::render(|buf| {
        crate::templates::index_html(buf, "CZ4R Home", admin)
    }))
}

async fn deactivatedworkers(
    State(pool): State<Pool<Postgres>>,
) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;

    Ok(crate::render(|buf| {
        crate::templates::deactivatedworkers_html(buf, "CZ4R Deleted Workers", admin)
    }))
}

async fn admin(State(pool): State<Pool<Postgres>>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;

    Ok(crate::render(|buf| {
        crate::templates::admin_html(buf, "CZ4R Admin Page", admin)
    }))
}

#[derive(Deserialize)]
struct WorkerEditForm {
    worker: Option<u64>,
}

async fn workeredit(
    State(pool): State<Pool<Postgres>>,
    Form(worker): Form<WorkerEditForm>,
) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;
    Ok(crate::render(|buf| {
        crate::templates::workeredit_html(buf, "CZ4R Worker Edit", admin, false, worker.worker)
    }))
}

async fn workercreate(State(pool): State<Pool<Postgres>>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;

    Ok(crate::render(|buf| {
        crate::templates::workeredit_html(buf, "CZ4R Worker Edit", admin, true, None)
    }))
}

#[derive(Deserialize)]
struct WorkerDataForm {
    worker: Option<u64>,
}

async fn workerdata(
    State(pool): State<Pool<Postgres>>,
    Form(worker): Form<WorkerDataForm>,
) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;

    Ok(crate::render(|buf| {
        crate::templates::workerdata_html(buf, "CZ4R Worker Data", admin, worker.worker)
    }))
}

async fn joblist(State(pool): State<Pool<Postgres>>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;

    Ok(crate::render(|buf| {
        crate::templates::joblist_html(buf, "CZ4R Job List", admin)
    }))
}

async fn jobedit(State(pool): State<Pool<Postgres>>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;

    Ok(crate::render(|buf| {
        crate::templates::jobedit_html(buf, "CZ4R Job Edit", admin, Some(12345))
    }))
}

async fn loginpage(State(pool): State<Pool<Postgres>>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;

    Ok(crate::render(|buf| {
        crate::templates::login_html(buf, "CZ4R Login")
    }))
}

async fn checkinout(State(pool): State<Pool<Postgres>>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;

    Ok(crate::render(|buf| {
        crate::templates::checkinout_html(
            buf,
            "CZ4R Time Tracking",
            admin,
            "Bank 123",
            "456 Main St.",
            "12/16/2023",
            "Do ya job or you dead!",
        )
    }))
}

async fn error404(State(pool): State<Pool<Postgres>>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;

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

// Include the generated source code
include!(concat!(env!("OUT_DIR"), "/templates.rs"));
