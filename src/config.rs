use std::env;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

use base64::Engine;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres};

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub site_url: String,
    pub login_secret: Vec<u8>,
    pub port: u16,
}

impl Config {
    pub fn new() -> Config {
        let database_url = env::var("DATABASE_URL").expect("DATABASE_URL not set");
        let login_secret_b64 = env::var("LOGIN_SECRET").expect("LOGIN_SECRET not set");
        let site_url = env::var("SITE_URL").expect("SITE_URL not set");
        let login_secret = base64::engine::general_purpose::STANDARD
            .decode(login_secret_b64)
            .expect("Invalid LOGIN_SECRET data");
        let port = env::var("CZ4R_ADDR")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3000u16);
        Config {
            database_url,
            site_url,
            login_secret,
            port,
        }
    }

    pub async fn create_pool(&self) -> Pool<Postgres> {
        PgPoolOptions::new()
            .connect(&self.database_url)
            .await
            .unwrap()
    }
}
