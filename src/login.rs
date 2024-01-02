use crate::AppState;
use crate::errors::CustomError;

use super::Worker;

use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum_template::RenderHtml;
use sqlx::query_as;
use crate::Backend;
use axum_login::AuthSession;
use axum::extract::State;
use axum::Form;
use password_hash::SaltString;
use scrypt::password_hash::PasswordHasher;
use scrypt::Scrypt;
use serde::Deserialize;
use sqlx::Pool;
use sqlx::Postgres;

#[derive(Deserialize, Clone)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct LoginPageForm {
    failure: Option<bool>,
}

pub async fn loginpage(
    State(AppState { pool: _, engine }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<LoginPageForm>,
) -> Result<impl IntoResponse, CustomError> {

    let logged_in = auth.user.is_some();
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);

    let data = serde_json::json!({
        "title": "CZ4R Login",
        "admin": admin,
        "logged_in": logged_in,
        "failure": form.failure == Some(true)
    });

    Ok(RenderHtml("login.hbs",engine,data))

}

pub(crate) async fn login(
    mut auth: AuthSession<Backend>,
    State(pool): State<Pool<Postgres>>,
    Form(login_form): Form<LoginForm>, //Extension(worker): Extension<Worker>
) -> Redirect {
    let LoginForm { username, password } = login_form;

    let mut conn = pool.acquire().await.unwrap();

    let worker = query_as!(Worker, "select * from users where name = $1", username)
        .fetch_one(&mut *conn)
        .await;

    let worker = if let Ok(w) = worker {
        w
    } else {
        return Redirect::to("/loginpage?failure=true");
    };

    if worker.deactivated {
        return Redirect::to("/loginpage?failure=true");
    }

    if worker.must_change_pw && worker.hash.is_empty() && worker.salt.is_empty() {
        return Redirect::to(format!("/change-pw?id={}", worker.id).as_str());
    }

    let salt = &worker.salt;
    let saltstr: Result<SaltString, password_hash::Error> = SaltString::from_b64(salt.as_str());
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
        Redirect::to("/loginpage?failure=true")
    } else {
        Redirect::to("/")
    }
}

pub(crate) async fn logout(mut auth: AuthSession<Backend>, State(_pool): State<Pool<Postgres>>) -> Redirect {
    if (auth.logout().await).is_ok() {
        Redirect::to("/")
    } else {
        Redirect::to("/loginpage?failure=true")
    }
}
