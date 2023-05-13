use std::env;
use std::str::FromStr;

use axum_login::secrecy::SecretVec;
use base64::Engine;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres};

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub login_secret: Vec<u8>,
}

impl Config {
    pub fn new() -> Config {
        let database_url = env::var("DATABASE_URL").expect("DATABASE_URL not set");
        let login_secret_b64 = env::var("LOGIN_SECRET").expect("LOGIN_SECRET not set");

        let login_secret = base64::engine::general_purpose::STANDARD
            .decode(login_secret_b64)
            .expect("Invalid LOGIN_SECRET data");
        Config {
            database_url,
            login_secret,
        }
    }

    pub async fn create_pool(&self) -> Pool<Postgres> {
        let pool = PgPoolOptions::new()
            .connect(&self.database_url)
            .await
            .unwrap();

        pool
    }
}
