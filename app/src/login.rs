use super::Worker;

use axum::response::Html;
use axum::response::Redirect;
use sqlx::query_as;

use super::Auth;
use axum::extract::State;
use axum::Form;
use password_hash::SaltString;
use scrypt::password_hash::PasswordHasher;
use scrypt::Scrypt;
use serde::Deserialize;
use sqlx::Pool;
use sqlx::Postgres;

#[derive(Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
}

pub(crate) async fn login(
    mut auth: Auth,
    State(pool): State<Pool<Postgres>>,
    Form(login_form): Form<LoginForm>, //Extension(worker): Extension<Worker>
) -> Redirect {
    let LoginForm { username, password } = login_form;

    let mut conn = pool.acquire().await.unwrap();

    let worker = query_as!(Worker, "select * from users where name = $1", username)
        .fetch_one(&mut conn)
        .await;

    let worker = if let Ok(w) = worker {
        w
    } else {
        return Redirect::to("/loginpage?failure=true");
    };

    if worker.must_change_pw && worker.hash.is_empty() && worker.salt.is_empty() {
        return Redirect::to(format!("/change-pw?id={}", worker.id).as_str());
    }

    let salt = &worker.salt;
    let saltstr = SaltString::from_b64(salt.as_str());
    let saltstr = if let Ok(s) = saltstr {
        s
    } else {
        return Redirect::to("/loginpage?failure=true");
    };

    let hash = Scrypt
        .hash_password(password.as_bytes(), saltstr.as_salt())
        .unwrap()
        .to_string();

    let mut failure = false;

    if worker.hash == hash {
        if worker.must_change_pw {
            //do not log in
            return Redirect::to(format!("/change-pw?id={}", worker.id).as_str());
        }
        auth.login(&worker).await.unwrap();
    } else {
        failure = true;
    }

    if failure {
        return Redirect::to("/loginpage?failure=true");
    } else {
        return Redirect::to("/");
    }
}

pub(crate) async fn logout(mut auth: Auth, State(pool): State<Pool<Postgres>>) -> Redirect {
    auth.logout().await;
    Redirect::to("/")
}
