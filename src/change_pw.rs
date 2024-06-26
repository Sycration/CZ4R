//Name=&Address=&Phone=&Email=&Hourly=&Mileage=&Drivetime=

use crate::errors::CustomError;
use crate::AppState;
use crate::Backend;
use axum::extract::Path;
use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::Form;
use axum_login::AuthSession;
use axum_template::RenderHtml;
use password_hash::PasswordHasher;
use password_hash::SaltString;
use rand::thread_rng;
use scrypt::Scrypt;
use serde::Deserialize;
use serde_json::json;
use sqlx::query;
use sqlx::Pool;
use sqlx::Postgres;

#[derive(Deserialize)]
pub(crate) struct ChangePwPageForm {
    id: Option<i64>,
    no_match: Option<bool>,
}

pub(crate) async fn change_pw_page(
    State(AppState { pool: _, engine }): State<AppState>,
    mut auth: AuthSession<Backend>,
    Form(form): Form<ChangePwPageForm>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    let logged_in = auth.user.is_some();

    let admin = auth.user.as_ref().map_or(false, |w| w.admin);

    let id = if let Some(id) = auth.user.map(|u| u.id) {
        id
    } else if let Some(id) = form.id {
        id
    } else {
        return Err(CustomError::Auth(
            "Not logged in and no ID selected.".to_string(),
        ).build(&engine));
    };

    let data = json!({
        "title": "CZ4R Login",
        "admin": admin,
        "logged_in": logged_in,
        "failure": form.no_match == Some(true),
        "chg_id": id
    });

    Ok(RenderHtml("changepw.hbs", engine, data))
}

#[derive(Deserialize)]
pub(crate) struct ChangePwForm {
    password1: String,
    password2: String,
}

pub(crate) async fn change_pw(
State(AppState { pool, engine }): State<AppState>,
    mut _auth: AuthSession<Backend>,
    Path(id): Path<i64>,
    Form(form): Form<ChangePwForm>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    let must_change = query!(
        r#"
    select (must_change_pw) from users 
    where id = $1
    and users.deactivated = false;
    "#,
        id
    )
    .fetch_one(&pool)
    .await;
    let must_change = if let Ok(m) = must_change {
        m.must_change_pw
    } else {
        return Err(CustomError::Database(format!(
            "Nonsense data from database on user {}",
            id
        )).build(&engine));
    };

    if !must_change {
        return Err(CustomError::Auth(format!(
            "User {} cannot change their password right now. Nice try.",
            id
        )).build(&engine));
    }

    if form.password1 != form.password2 {
        return Ok(Redirect::to("/change-pw?no_match=true"));
    }

    let salt = SaltString::generate(&mut thread_rng());

    let hash = Scrypt
        .hash_password(form.password1.as_bytes(), salt.as_salt())
        .unwrap()
        .to_string();

    let res = query!(
        r#"
    update users
    set
    hash = $1,
    salt = $2,
    must_change_pw = false
    where id = $3;"#,
        hash,
        salt.as_str(),
        id
    )
    .execute(&pool)
    .await;
    if let Err(err) = res {
        return Err(CustomError::Database(err.to_string()).build(&engine));
    }

    Ok(Redirect::to("/loginpage"))
}
