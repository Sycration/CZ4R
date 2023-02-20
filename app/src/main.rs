mod config;
mod errors;

use crate::errors::CustomError;
use axum::{extract::{Extension, Path}, response::Html, routing::get, Router};
use deadpool_postgres::Pool;
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let config = config::Config::new();

    let pool = config.create_pool();

    // build our application with a route
    let app = Router::new()
        .route("/", get(index))
        .route("/joblist", get(joblist))
        .route("/jobedit", get(jobedit))
        .route("/loginpage", get(loginpage))
        .route("/checkinout", get(checkinout))
        .route("/admin", get(admin))
        .route("/admin/worker-edit", get(workereditblank))
        .route("/admin/worker-edit/:id", get(workeredit))
        .route("/admin/worker-create", get(workercreate))
        .fallback(error404)
        .layer(Extension(config))
        .layer(Extension(pool.clone()));

    // run it
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn index(Extension(pool): Extension<Pool>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;

    Ok(crate::render(|buf| {
        crate::templates::index_html(buf, "CZ4R Home", admin)
    }))
}

async fn admin(Extension(pool): Extension<Pool>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;

    Ok(crate::render(|buf| {
        crate::templates::admin_html(buf, "CZ4R Admin Page", admin)
    }))
}

async fn workeredit(id: Path<u64>, Extension(pool): Extension<Pool>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;
    Ok(crate::render(|buf| {
        crate::templates::workeredit_html(buf, "CZ4R Admin Page", admin, false, Some(id.0))
    }))
}
async fn workereditblank(Extension(pool): Extension<Pool>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;
    Ok(crate::render(|buf| {
        crate::templates::workeredit_html(buf, "CZ4R Admin Page", admin, false, None)
    }))
}

async fn workercreate(Extension(pool): Extension<Pool>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;

    Ok(crate::render(|buf| {
        crate::templates::workeredit_html(buf, "CZ4R Admin Page", admin, true, None)
    }))
}

async fn joblist(Extension(pool): Extension<Pool>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;

    Ok(crate::render(|buf| {
        crate::templates::joblist_html(buf, "CZ4R Job List", admin)
    }))
}

async fn jobedit(Extension(pool): Extension<Pool>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;

    Ok(crate::render(|buf| {
        crate::templates::jobedit_html(buf, "CZ4R Job Edit", admin, Some(12345))
    }))
}

async fn loginpage(Extension(pool): Extension<Pool>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;


    Ok(crate::render(|buf| {
        crate::templates::login_html(buf, "CZ4R Login")
    }))
}

async fn checkinout(Extension(pool): Extension<Pool>) -> Result<Html<String>, CustomError> {
    //let client = pool.get().await?;

    //let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;
    let admin = true;

    Ok(crate::render(|buf| {
        crate::templates::checkinout_html(
            buf,
            "CZ4R Time Tracking", admin,
            "Bank 123",
            "456 Main St.",
            "12/16/2023",
            "Do ya job or you dead!",
        )
    }))
}

async fn error404(Extension(pool): Extension<Pool>) -> Result<Html<String>, CustomError> {
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
include!(concat!(env!("OUT_DIR"), "/cornucopia.rs"));
include!(concat!(env!("OUT_DIR"), "/templates.rs"));
