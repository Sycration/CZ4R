use std::env;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::thread::JoinHandle;
use std::time::Duration;

use aws_config::meta::region::RegionProviderChain;
use aws_config::BehaviorVersion;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::Client;
use base64::Engine;
use futures::channel::oneshot;
use futures::task;
use password_hash::{PasswordHasher, Salt, SaltString};

use rand::thread_rng;
use scrypt::Scrypt;
use sqlx::migrate::MigrateDatabase;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{query, Pool, Sqlite};
use tokio::io::AsyncWriteExt;
use tracing::{error, warn};

#[derive(Debug)]
pub struct Config {
    pub database_url: String,
    pub site_url: String,
    pub login_secret: Vec<u8>,
    pub port: u16,
    pub backup_task: Option<tokio::task::JoinHandle<()>>,
}

impl Config {
    pub async fn new() -> Config {
        dotenvy::dotenv();
        let database_url = env::var("DATABASE_URL").expect("DATABASE_URL not set");

        let mut doing_backups = false;
        let (send_pool, recv_pool) = oneshot::channel();
        let mut backup_task = None;

        if let Ok(region) = env::var("AWS_REGION") {
            if let Ok(bucket_name) = env::var("AWS_BUCKET") {
                if let Ok(Ok(backup_time)) = env::var("AWS_BACKUP_TIME").map(|s| s.parse::<u64>()) {
                    doing_backups = true;
                    let database_url = database_url.clone();

                    let mut interval = tokio::time::interval(Duration::from_secs(backup_time));

                    let region_provider = RegionProviderChain::first_try(Region::new(region));
                    let region = region_provider.region().await.unwrap();
                    let shared_config = aws_config::defaults(BehaviorVersion::latest())
                        .region(region_provider)
                        .load()
                        .await;
                    let client = Client::new(&shared_config);

                    let mut url = url::Url::parse(&database_url).expect("Invalid database URL");

                    let mut path = url.host().map(|h|h.to_string()).unwrap_or_default();
                    let path_part = url.path();

                    path.push_str(path_part);

                    let file_name = path.split('/').last().unwrap();
                    dbg!(&path);
                    dbg!(tokio::fs::try_exists(&path).await);
                    if !sqlx::Sqlite::database_exists(&database_url).await.unwrap() {
                        if let Ok(mut output) = client
                            .get_object()
                            .bucket(&bucket_name)
                            .key(file_name)
                            .send()
                            .await
                        {
                            if let Ok(mut file) = tokio::fs::File::create(&path).await {
                                let mut bytes = output.body.collect().await.unwrap();
                                file.write_all_buf(&mut bytes).await.unwrap();
                            }
                        }
                    }
                    backup_task = Some(tokio::task::spawn({
                        let file_name = file_name.clone().to_string();
                        async move {
                            let backup_pool = recv_pool.await.unwrap();

                            interval.tick().await;
                            loop {
                                match aws_sdk_s3::primitives::ByteStream::from_path(&path).await {
                                    Ok(contents) => {
                                        let upload = client
                                            .put_object()
                                            .bucket(&bucket_name)
                                            .key(&file_name)
                                            .body(contents)
                                            .send()
                                            .await;
                                        if let Err(e) = upload {
                                            error!("{}", e.to_string());
                                        }
                                    }
                                    Err(e) => error!("{}", e.to_string()),
                                }

                                interval.tick().await;
                            }
                        }
                    }));
                }
            }
        }

        let login_secret_b64 = env::var("LOGIN_SECRET").expect("LOGIN_SECRET not set");
        let site_url = env::var("SITE_URL").expect("SITE_URL not set");
        let login_secret = base64::engine::general_purpose::STANDARD
            .decode(login_secret_b64)
            .expect("Invalid LOGIN_SECRET data");
        let port = env::var("CZ4R_ADDR")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3000u16);
        let config = Config {
            database_url,
            site_url,
            login_secret,
            port,
            backup_task,
        };

        let config_pool = config.create_pool().await;

        sqlx::migrate!("./migrations")
            .run(&config_pool)
            .await
            .unwrap();

        //check if no users, create it from env vars otherwise
        let a = query!("select count(id) as count from users;")
            .fetch_one(&config_pool)
            .await
            .unwrap()
            .count;
        if a == 0 {
            let admin_uname = env::var("ADMIN_USER").expect("ADMIN_USER not set");
            let admin_pw = env::var("ADMIN_PASSWORD").expect("ADMIN_PASSWORD not set");

            let salt = SaltString::generate(&mut thread_rng());

            let hash = Scrypt
                .hash_password(admin_pw.as_bytes(), salt.as_salt())
                .unwrap()
                .to_string();
            use password_hash::SaltString;

            let salt_str = salt.as_str();
            query!(
                "insert into users (name, hash, salt, admin) values ($1, $2, $3, $4);",
                admin_uname,
                hash,
                salt_str,
                true
            )
            .execute(&config_pool)
            .await
            .expect("Failed to insert default admin user");
        }

        if doing_backups {
            send_pool.send(config_pool).unwrap();
        }
        return config;
    }

    pub async fn create_pool(&self) -> Pool<Sqlite> {
        let options = SqliteConnectOptions::from_str(&self.database_url)
            .unwrap()
            .create_if_missing(true)
            .pragma("journal_mode", "DELETE");
        SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .unwrap()
    }
}
