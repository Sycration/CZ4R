mod config;
mod errors;

use crate::errors::CustomError;
use axum::{extract::Extension, response::Html, routing::get, Router};
use deadpool_postgres::Pool;
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let config = config::Config::new();

    let pool = config.create_pool();

    // build our application with a route
    let app = Router::new()
        .route("/", get(fortunes))
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

async fn fortunes(Extension(pool): Extension<Pool>) -> Result<Html<&'static str>, CustomError> {
    let client = pool.get().await?;

    let fortunes = queries::fortunes::fortunes().bind(&client).all().await?;

    Ok(crate::render(|buf| {
        crate::templates::index_html(buf, "Fortunes", fortunes)
    }))
}

pub fn render<F>(f: F) -> Html<&'static str>
where
    F: FnOnce(&mut Vec<u8>) -> Result<(), std::io::Error>,
{
    let mut buf = Vec::new();
    f(&mut buf).expect("Error rendering template");
    let html: String = String::from_utf8_lossy(&buf).into();

    Html(Box::leak(html.into_boxed_str()))
}

// Include the generated source code
include!(concat!(env!("OUT_DIR"), "/cornucopia.rs"));
include!(concat!(env!("OUT_DIR"), "/templates.rs"));
