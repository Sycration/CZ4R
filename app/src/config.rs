use std::env;
use std::str::FromStr;

use sqlx::{Pool, Postgres};
use sqlx::postgres::PgPoolOptions;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
}

impl Config {
    pub fn new() -> Config {
        let database_url = env::var("DATABASE_URL").expect("DATABASE_URL not set");

        Config { database_url }
    }

    pub async fn create_pool(&self) -> Pool<Postgres> {
        let pool = PgPoolOptions::new().connect(&self.database_url).await.unwrap();


        pool
    }
}
