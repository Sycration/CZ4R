use std::convert::Infallible;

use crate::errors::CustomError;
use crate::get_admin;
use crate::AppState;

use super::Worker;

use axum_login::AuthnBackend;
use sqlx::query;
use sqlx::Row;
use crate::Backend;
use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::Form;
use axum_login::tower_sessions::Session;
use axum_login::AuthSession;
use axum_template::RenderHtml;
use password_hash::SaltString;
use scrypt::password_hash::PasswordHasher;
use scrypt::Scrypt;
use serde::Deserialize;
use sqlx::query_as;
use sqlx::Pool;
use sqlx::Sqlite;
use tracing::debug;
use tracing::info;
use tracing::trace;
use tracing::warn;

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
    State(AppState {
        pool: _, engine, ..
    }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<LoginPageForm>,
) -> Result<impl IntoResponse, Infallible> {
    let logged_in = auth.user.is_some();
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);

    let data = serde_json::json!({
        "title": "CZ4R Login",
        "admin": admin,
        "logged_in": logged_in,
        "failure": form.failure == Some(true)
    });

    Ok(RenderHtml("login.hbs", engine, data))
}

pub(crate) async fn login(
    mut auth: AuthSession<Backend>,
   // mut session: Session,
    State(AppState {
        pool, engine: _, ..
    }): State<AppState>,
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
        debug!(
            "user {} can't log in because there is nobody with that name in the database",
            &username
        );
        return Redirect::to("/loginpage?failure=true");
    };

    if worker.deactivated {
        debug!(
            "user {} (id {}) can't log in because they are deactivated",
            &worker.name, &worker.id
        );
        return Redirect::to("/loginpage?failure=true");
    }

    if worker.must_change_pw {
        debug!(
            "user {} (id {}) has to change their password",
            &worker.name, &worker.id
        );
        return Redirect::to(format!("/change-pw?id={}", worker.id).as_str());
    }

    let salt = &worker.salt;
    let saltstr: Result<SaltString, password_hash::Error> = SaltString::from_b64(salt.as_str());
    let saltstr = if let Ok(s) = saltstr {
        s
    } else {
        warn!(
            "user {} (id {}) can't log in because they have an invalid salt",
            &worker.name, &worker.id
        );
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
            debug!(
                "user {} (id {}) has to change their password",
                &worker.name, &worker.id
            );
            return Redirect::to(format!("/change-pw?id={}", worker.id).as_str());
        }
        auth.login(&worker).await.unwrap();
        let id = worker.id;
        query!(r#"
        update users
            set logged_out = 0
            where id = $1
        "#, id).execute(&pool).await.unwrap();
        
        info!("user {} (id {}) has logged in", &worker.name, &worker.id);
    } else {
        failure = true;
        debug!(
            "user {} (id {}) can't log in because they used the wrong password",
            &worker.name, &worker.id
        );
    }

    if failure {
        Redirect::to("/loginpage?failure=true")
    } else {
        Redirect::to("/")
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct LogoutForm {
     pub id: i64
}

#[axum::debug_handler]
pub async fn logout_user(
    State(AppState { pool, .. }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<LogoutForm>,
) -> Result<impl IntoResponse, CustomError> {
    let (my_id, my_name) = get_admin(&auth)?;

    let me = (&auth).user.clone().unwrap();
    let user = &auth.backend.get_user(&form.id).await?;

    if let Some(u) = user {

        
        query!(r#"
        update users
            set logged_out = 1
            where id = $1
        "#, u.id).execute(&pool).await?;
    }

    


    Ok(())

}

pub(crate) async fn logout(
    mut auth: AuthSession<Backend>,
   // mut session: Session,
    State(_pool): State<Pool<Sqlite>>,
) -> Redirect {
    if let Some(user) = auth.user.clone() {
        if (auth.logout().await).is_ok() /*&& session.flush().await.is_ok() */{
            debug!("user {} (id {}) logged out", user.name, user.id);
            Redirect::to("/")
        } else {
            warn!("user {} (id {}) could not log out", user.name, user.id);

            Redirect::to("/loginpage?failure=true")
        }
    } else {
        debug!("user who is not logged in attempted to log out");

        Redirect::to("/loginpage?failure=true")
    }
}
