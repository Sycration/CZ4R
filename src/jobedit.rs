use std::collections::HashMap;

use axum::{
    extract::State,
    response::{IntoResponse, Redirect},
    Form, Json,
};
use axum_template::RenderHtml;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{query, query_as, types::time::Date, Pool, QueryBuilder, Sqlite};
use std::result::Result::Ok;
use tracing::{info, trace};

use crate::errors::{to_api, to_api_with_status, ApiError};
use crate::{api_auth::ApiAuth, current_user, errors::CustomError, AppState, Job};
use crate::{get_admin, Backend};
use axum::http::StatusCode;
use axum_login::AuthSession;
use git_version::git_version;
use utoipa::ToSchema;

#[derive(Deserialize)]
pub(crate) struct JobEditPage {
    id: Option<i64>,
}

/// A simplified view of a [`Job`] suitable for JSON responses (dates as
/// strings rather than `time::Date`).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct JobDetail {
    pub id: i64,
    pub sitename: String,
    pub workorder: String,
    pub servicecode: String,
    pub address: String,
    pub date: String,
    pub notes: String,
}

impl From<Job> for JobDetail {
    fn from(job: Job) -> Self {
        Self {
            id: job.id,
            sitename: job.sitename,
            workorder: job.workorder,
            servicecode: job.servicecode,
            address: job.address,
            date: job.date.to_string(),
            notes: job.notes,
        }
    }
}

/// A worker, and whether/how they're assigned to the job being edited.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct WorkerAssignmentInfo {
    pub id: i64,
    pub name: String,
    pub assigned: bool,
    pub flat_rate: bool,
}

/// The data needed to render (or serve as JSON) the job-edit page: the job
/// itself (`None` when creating a new job) and every active worker along
/// with their assignment status on it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct JobEditPageOutput {
    pub job: Option<JobDetail>,
    pub workers: Vec<WorkerAssignmentInfo>,
}

/// Shared business logic for both the HTML job-edit page and its JSON API
/// equivalent.
async fn jobeditpage_core(
    pool: &Pool<Sqlite>,
    user: Option<&crate::CurrentUser>,
    id: Option<i64>,
) -> Result<JobEditPageOutput, CustomError> {
    get_admin(user)?;

    let this_job = match id {
        Some(id) => Some(
            query_as!(Job, "select * from jobs where id = $1", id)
                .fetch_optional(pool)
                .await?
                .ok_or_else(|| {
                    CustomError::new(anyhow::anyhow!("No job with id {id}"), StatusCode::NOT_FOUND)
                })?,
        ),
        None => None,
    };

    let workers = query!("select id, name from users where users.deactivated = false;")
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| (r.id, r.name))
        .collect::<Vec<_>>();

    let assigned_fr: HashMap<i64, bool> = match id {
        Some(id) => query!(
            r#"select users.id, jobworkers.using_flat_rate from users
        inner join jobworkers
        on users.id = jobworkers.worker
        where jobworkers.job = $1
        and users.deactivated = false;
        "#,
            id
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .fold(HashMap::new(), |mut acc, x| {
            acc.entry(x.id).or_insert(x.using_flat_rate);
            acc
        }),
        None => HashMap::new(),
    };

    let workers = workers
        .into_iter()
        .map(|(id, name)| WorkerAssignmentInfo {
            id,
            name,
            assigned: assigned_fr.contains_key(&id),
            flat_rate: assigned_fr.get(&id).copied().unwrap_or(false),
        })
        .collect::<Vec<_>>();

    Ok(JobEditPageOutput {
        job: this_job.map(JobDetail::from),
        workers,
    })
}

/// `GET /admin/jobedit` — HTML-facing endpoint.
pub(crate) async fn jobeditpage(
    State(AppState { pool, engine, .. }): State<AppState>,
    auth: AuthSession<Backend>,
    Form(form): Form<JobEditPage>,
) -> Result<impl IntoResponse, CustomError> {
    let out = jobeditpage_core(&pool, current_user(&auth).as_ref(), form.id).await?;

    let data = json!({
    "git_ver": git_version!(),
        "title": "Job Edit",
        "admin": true,
        "logged_in": true,
        "job": out.job,
        "list-data": out.workers
    });

    Ok(RenderHtml("jobedit.hbs", engine, data))
}

/// `GET /admin/api/v1/jobs/{id}` — REST/JSON endpoint. See
/// [`jobeditpage_api_new`] for the "new job" form-data equivalent.
#[utoipa::path(
    get,
    path = "/admin/api/v1/jobs/{id}",
    params(("id" = i64, Path, description = "Job id")),
    responses((status = OK, body = JobEditPageOutput)),
    security(("bearer_auth" = [])),
    tag = super::ADMIN_TAG
)]
pub(crate) async fn jobeditpage_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Json<JobEditPageOutput>, ApiError> {
    to_api(jobeditpage_core(&pool, Some(&user), Some(id)).await)
}

/// `GET /admin/api/v1/jobs/new` — the data needed to build a "create job"
/// form: no job, but the full active-worker list.
#[utoipa::path(
    get,
    path = "/admin/api/v1/jobs/new",
    responses((status = OK, body = JobEditPageOutput)),
    security(("bearer_auth" = [])),
    tag = super::ADMIN_TAG
)]
pub(crate) async fn jobeditpage_api_new(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
) -> Result<Json<JobEditPageOutput>, ApiError> {
    to_api(jobeditpage_core(&pool, Some(&user), None).await)
}

/// A single worker assignment on a job, and whether that worker is being
/// paid a flat rate for it.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub(crate) struct JobAssignment {
    pub worker: i64,
    pub flat_rate: bool,
}

/// Request body shared by the web form and the JSON API for creating or
/// updating a job. `job_id` is `None` when creating a new job.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub(crate) struct JobEditInput {
    pub job_id: Option<i64>,
    pub sitename: String,
    pub servcode: String,
    pub workorder: String,
    pub address: String,
    pub date: Date,
    pub notes: String,
    pub assignments: Vec<JobAssignment>,
}

/// The legacy form fields the HTML client posts: `assigned` and `flatrate`
/// are dash-separated lists of worker ids (e.g. `"1-2-3"`), with `flatrate`
/// being the subset of `assigned` that uses a flat rate.
#[derive(Deserialize)]
pub(crate) struct JobEditForm {
    sitename: String,
    servcode: String,
    workorder: String,
    address: String,
    date: Date,
    assigned: String,
    flatrate: String,
    jobid: Option<i64>,
    notes: String,
}

impl From<JobEditForm> for JobEditInput {
    fn from(form: JobEditForm) -> Self {
        let to_flatrt = form
            .flatrate
            .split('-')
            .filter_map(|n| n.parse::<i64>().ok())
            .collect::<Vec<_>>();

        let assignments = form
            .assigned
            .split('-')
            .filter_map(|n| n.parse::<i64>().ok())
            .map(|worker| JobAssignment {
                worker,
                flat_rate: to_flatrt.contains(&worker),
            })
            .collect::<Vec<_>>();

        Self {
            job_id: form.jobid,
            sitename: form.sitename,
            servcode: form.servcode,
            workorder: form.workorder,
            address: form.address,
            date: form.date,
            notes: form.notes,
            assignments,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct JobEditOutput {
    pub job_id: i64,
}

async fn jobedit_core(
    pool: &Pool<Sqlite>,
    user: Option<&crate::CurrentUser>,
    input: JobEditInput,
) -> Result<JobEditOutput, CustomError> {
    let (my_id, my_name) = get_admin(user)?;

    // De-duplicate by worker id: last one wins if the same worker somehow
    // appears twice in the request.
    let to_assign: HashMap<i64, bool> = input
        .assignments
        .iter()
        .map(|a| (a.worker, a.flat_rate))
        .collect();

    if let Some(job_id) = input.job_id {
        let mut tx = pool.begin().await?;

        //update job itself
        query!(
            r#"
        update jobs set 
            sitename = $2,
            workorder = $3,
            servicecode = $4,
            address = $5,
            date = $6,
            notes = $7
        where id = $1;"#,
            job_id,
            input.sitename,
            input.workorder,
            input.servcode,
            input.address,
            input.date,
            input.notes
        )
        .execute(&mut *tx)
        .await?;

        let currently_assigned: HashMap<i64, bool> = query!(
            r#"
        select jobworkers.worker, jobworkers.using_flat_rate
            from
        jobworkers inner join jobs
            on jobworkers.job = jobs.id
        where jobs.id = $1;"#,
            job_id
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|v| (v.worker, v.using_flat_rate))
        .collect();

        // Workers currently assigned to the job but no longer wanted: drop
        // their `jobworkers` row entirely.
        let assignments_to_remove: Vec<i64> = currently_assigned
            .keys()
            .filter(|worker| !to_assign.contains_key(worker))
            .copied()
            .collect();

        // Workers that should stay assigned, but whose flat-rate flag
        // changed: update in place (never delete+re-insert, which is what
        // caused duplicate `jobworkers` rows/duplicate job-list entries in
        // the past).
        let flatrate_changes: Vec<(i64, bool)> = to_assign
            .iter()
            .filter_map(|(worker, flat_rate)| {
                match currently_assigned.get(worker) {
                    Some(current_flat_rate) if current_flat_rate != flat_rate => {
                        Some((*worker, *flat_rate))
                    }
                    _ => None,
                }
            })
            .collect();

        // Workers newly assigned to the job.
        let assignments_to_add: Vec<(i64, bool)> = to_assign
            .iter()
            .filter(|(worker, _)| !currently_assigned.contains_key(worker))
            .map(|(worker, flat_rate)| (*worker, *flat_rate))
            .collect();

        //remove assignments that are no longer wanted
        if !assignments_to_remove.is_empty() {
            let mut query_builder: QueryBuilder<Sqlite> =
                QueryBuilder::new("delete from jobworkers where job = ");
            query_builder.push_bind(job_id).push(" and worker in (");
            let mut separated = query_builder.separated(", ");
            for worker in &assignments_to_remove {
                separated.push_bind(worker);
            }
            separated.push_unseparated(")");
            query_builder.build().execute(&mut *tx).await?;
            trace!(
                "removed assignments on job {} for users {:?}",
                job_id,
                &assignments_to_remove
            );
        }

        //update flat-rate flags for workers that stay assigned
        for (worker, flat_rate) in &flatrate_changes {
            query!(
                "update jobworkers set using_flat_rate = $1 where job = $2 and worker = $3;",
                flat_rate,
                job_id,
                worker
            )
            .execute(&mut *tx)
            .await?;
        }
        if !flatrate_changes.is_empty() {
            trace!(
                "updated flat-rate flags on job {} for users {:?}",
                job_id,
                &flatrate_changes
            );
        }

        //create assignments w/ flatrates
        if !assignments_to_add.is_empty() {
            let mut query_builder: QueryBuilder<Sqlite> =
                QueryBuilder::new("insert into jobworkers (job, worker, using_flat_rate) ");
            query_builder.push_values(assignments_to_add.iter().take(250), |mut b, assignment| {
                b.push_bind(job_id)
                    .push_bind(assignment.0)
                    .push_bind(assignment.1);
            });
            let query = query_builder.build();
            query.execute(&mut *tx).await?;
            trace!(
                "added assignments on job {} for users {:?}\n flat-rates on {:?}",
                job_id,
                &assignments_to_add.iter().map(|x| x.0).collect::<Vec<_>>(),
                &assignments_to_add
                    .iter()
                    .filter(|x| x.1)
                    .map(|x| x.0)
                    .collect::<Vec<_>>(),
            );
        }

        tx.commit().await?;

        info!(
            "admin {my_name} (id {my_id}) updated job {job_id}:\n
site name: {}\n
workorder: {}\n
service code: {}\n
address: {}\n
date: {}\n
notes: {}",
            input.sitename, input.workorder, input.servcode, input.address, input.date, input.notes
        );

        Ok(JobEditOutput { job_id })
    } else {
        let mut tx = pool.begin().await?;

        //create job
        let job_id: i64 = query!(
            r#"
        insert into jobs (sitename, workorder, servicecode, address, date, notes) values
                ($1, $2, $3, $4, $5, $6)
            returning id;"#,
            input.sitename,
            input.workorder,
            input.servcode,
            input.address,
            input.date,
            input.notes
        )
        .fetch_one(&mut *tx)
        .await?
        .id;

        info!(
            "admin {my_name} (id {my_id}) created job {job_id}:\n
site name: {}\n
workorder: {}\n
service code: {}\n
address: {}\n
date: {}\n
notes: {}",
            input.sitename, input.workorder, input.servcode, input.address, input.date, input.notes
        );

        //create assignments w/ flatrates
        if !to_assign.is_empty() {
            let mut query_builder: QueryBuilder<Sqlite> =
                QueryBuilder::new("insert into jobworkers (job, worker, using_flat_rate) ");
            query_builder.push_values(to_assign.iter().take(250), |mut b, assignment| {
                b.push_bind(job_id)
                    .push_bind(assignment.0)
                    .push_bind(assignment.1);
            });

            let query = query_builder.build();
            query.execute(&mut *tx).await?;
        }

        tx.commit().await?;
        Ok(JobEditOutput { job_id })
    }
}

/// `POST /admin/web/v1/edit-job` — HTML-facing endpoint.
pub(crate) async fn jobedit(
    State(AppState { pool, .. }): State<AppState>,
    auth: AuthSession<Backend>,
    Form(form): Form<JobEditForm>,
) -> Result<impl IntoResponse, CustomError> {
    let out = jobedit_core(&pool, current_user(&auth).as_ref(), form.into()).await?;
    Ok(Redirect::to(format!("/admin/jobedit?id={}", out.job_id).as_str()))
}

/// `POST /admin/api/v1/jobs` — REST/JSON endpoint that creates a job.
/// `job_id` in the body is ignored (a new job always gets a fresh id).
/// Responds `201 Created`.
#[utoipa::path(
    post,
    path = "/admin/api/v1/jobs",
    request_body = JobEditInput,
    responses((status = CREATED, body = JobEditOutput)),
    security(("bearer_auth" = [])),
    tag = super::ADMIN_TAG
)]
pub(crate) async fn create_job_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
    Json(mut input): Json<JobEditInput>,
) -> Result<(StatusCode, Json<JobEditOutput>), ApiError> {
    input.job_id = None;
    to_api_with_status(
        jobedit_core(&pool, Some(&user), input).await,
        StatusCode::CREATED,
    )
}

/// `PUT /admin/api/v1/jobs/{id}` — REST/JSON endpoint that updates a job.
/// The id comes from the path, not the body.
#[utoipa::path(
    put,
    path = "/admin/api/v1/jobs/{id}",
    params(("id" = i64, Path, description = "Job id")),
    request_body = JobEditInput,
    responses((status = OK, body = JobEditOutput)),
    security(("bearer_auth" = [])),
    tag = super::ADMIN_TAG
)]
pub(crate) async fn update_job_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(mut input): Json<JobEditInput>,
) -> Result<Json<JobEditOutput>, ApiError> {
    input.job_id = Some(id);
    to_api(jobedit_core(&pool, Some(&user), input).await)
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub(crate) struct JobDeleteInput {
    pub job_id: i64,
}

#[derive(Deserialize)]
pub(crate) struct JobDeleteForm {
    jobid: i64,
}

impl From<JobDeleteForm> for JobDeleteInput {
    fn from(form: JobDeleteForm) -> Self {
        Self { job_id: form.jobid }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct JobDeleteOutput {
    pub job_id: i64,
}

async fn jobdelete_core(
    pool: &Pool<Sqlite>,
    user: Option<&crate::CurrentUser>,
    input: JobDeleteInput,
) -> Result<JobDeleteOutput, CustomError> {
    let (my_id, my_name) = get_admin(user)?;

    query!(
        r#"
    delete from jobworkers
        where 
        job = $1;
    "#,
        input.job_id
    )
    .execute(pool)
    .await?;

    query!(
        r#"
    delete from jobs
        where 
        id = $1;
    "#,
        input.job_id
    )
    .execute(pool)
    .await?;

    info!("admin {} (id {}) deleted job {}", my_name, my_id, input.job_id);

    Ok(JobDeleteOutput { job_id: input.job_id })
}

/// `POST /admin/web/v1/delete-job` — HTML-facing endpoint.
pub(crate) async fn jobdelete(
    State(AppState { pool, .. }): State<AppState>,
    auth: AuthSession<Backend>,
    Form(form): Form<JobDeleteForm>,
) -> Result<impl IntoResponse, CustomError> {
    jobdelete_core(&pool, current_user(&auth).as_ref(), form.into()).await?;
    Ok(Redirect::to("/joblist"))
}

/// `DELETE /admin/api/v1/jobs/{id}` — REST/JSON endpoint.
#[utoipa::path(
    delete,
    path = "/admin/api/v1/jobs/{id}",
    params(("id" = i64, Path, description = "Job id")),
    responses((status = OK, body = JobDeleteOutput)),
    security(("bearer_auth" = [])),
    tag = super::ADMIN_TAG
)]
pub(crate) async fn delete_job_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Json<JobDeleteOutput>, ApiError> {
    to_api(jobdelete_core(&pool, Some(&user), JobDeleteInput { job_id: id }).await)
}
