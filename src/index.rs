use std::collections::HashMap;
use std::convert::Infallible;
use std::env;

use crate::api_auth::ApiAuth;
use crate::errors::{to_api, ApiError};
use crate::workerdata::hours_worked;
use crate::{current_user, get_admin, now, Backend};
use crate::{errors::CustomError, AppEngine, AppState, Job, JobWorker};
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect},
    Form, Json,
};
use axum_login::AuthSession;
use axum_template::RenderHtml;
use git_version::git_version;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{query, query_as, query_scalar, Pool, Row, Sqlite};
use time::format_description::well_known::Iso8601;
use time::{Date, Duration, Time};
use utoipa::ToSchema;

/// The summary statistics shown on the homepage: total jobs/miles/workers
/// tracked, and a few rough daily/monthly averages. This is public
/// information (shown to logged-out visitors too), so unlike most of the
/// other `/api/v1/*` endpoints, this one requires no authentication.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct HomeStats {
    pub jobs: i64,
    pub jobs_avg: String,
    pub miles: String,
    pub miles_avg: String,
    pub workers: i64,
    pub workers_avg: String,
    pub site_url: String,
}

/// Shared business logic for both the homepage and its JSON API
/// equivalent. This never actually fails - every query has a sensible
/// fallback - so unlike most `*_core` functions elsewhere, this one isn't
/// fallible.
async fn index_core(pool: &Pool<Sqlite>) -> HomeStats {
    let jobs = query_scalar!(
        r#"
        select count(*) from jobs;   
    "#
    )
    .fetch_one(pool)
    .await;
    let jobs = match jobs {
        Ok(v) => v,
        _ => 0,
    };

    let workers = query_scalar!(
        r#"
        select count(*) from users where admin = false and deactivated = false;   
    "#
    )
    .fetch_one(pool)
    .await;
    let workers = match workers {
        Ok(v) => v,
        _ => 0,
    };

    let miles = query_scalar!(
        r#"
        select sum(miles_driven) from jobworkers;   
    "#
    )
    .fetch_one(pool)
    .await;
    let miles = match miles {
        Ok(Some(v)) => v,
        _ => 0.0,
    };

    let earliest: Result<time::Date, sqlx::Error> = query_scalar!(
        r#"
        select date from jobs order by date asc;   
    "#
    )
    .fetch_one(pool)
    .await;
    let days = match earliest {
        Ok(v) => (time::OffsetDateTime::now_utc().date() - v).whole_days(),
        _ => 0,
    };

    let mut jobsavg = days as f64 / jobs as f64;
    if !jobsavg.is_finite() {
        jobsavg = 0.0
    };

    let mut milesavg = miles as f64 / days as f64;
    if !milesavg.is_finite() {
        milesavg = 0.0
    };

    let mut workersavg = (days as f64 / 30.437) / workers as f64;
    if !workersavg.is_finite() {
        workersavg = 0.0
    };

    HomeStats {
        jobs,
        jobs_avg: format!("{:.2}", jobsavg),
        miles: format!("{:.2}", miles),
        miles_avg: format!("{:.2}", milesavg),
        workers,
        workers_avg: format!("{:.2}", workersavg),
        site_url: env::var("SITE_URL").expect("SITE_URL not set"),
    }
}

/// `GET /` — HTML-facing endpoint.
pub(crate) async fn index(
    State(AppState { pool, engine, .. }): State<AppState>,

    auth: AuthSession<Backend>,
) -> Result<impl IntoResponse, Infallible> {
    let admin = auth.user.as_ref().map_or(false, |w| w.admin);
    let logged_in = auth.user.is_some();

    let stats = index_core(&pool).await;

    // Admins get the detailed dashboard instead of the three summary cards.
    // If it fails for any reason the page still renders, with the cards.
    let dash = if admin {
        match dashboard_core(&pool, current_user(&auth).as_ref()).await {
            Ok(d) => serde_json::to_value(d).ok(),
            Err(e) => {
                tracing::warn!("failed to build admin dashboard: {e:?}");
                None
            }
        }
    } else {
        None
    };

    let data = serde_json::json!({
        "dash": dash,
    "git_ver": git_version!(),
        "admin": admin,
        "logged_in": logged_in,
        "title": "CZ4R",
        "jobs": stats.jobs,
        "jobsavg": stats.jobs_avg,
        "miles": stats.miles,
        "milesavg": stats.miles_avg,
        "workers": stats.workers,
        "workersavg": stats.workers_avg,
        "siteurl": stats.site_url
    });

    Ok(RenderHtml("home.hbs", engine, data))
}

/// Some public statistics, does not require authentication.
#[utoipa::path(
    get,
    path = "/api/v1/stats",
    responses((status = OK, body = HomeStats)),
    tag = super::USER_TAG
)]
pub(crate) async fn index_api(
    State(AppState { pool, .. }): State<AppState>,
) -> Json<HomeStats> {
    Json(index_core(&pool).await)
}


// ---------------------------------------------------------------------
// Admin dashboard
// ---------------------------------------------------------------------

/// How many rows each "needs attention" / list section returns at most.
const DASH_LIST_LIMIT: usize = 15;

/// Headline numbers. "This month" runs from the 1st through today;
/// "last month" is the whole previous calendar month. Hours, miles and
/// expenses only count assignments that have both a sign-in and a
/// sign-out, and hours use the same minimum-one-hour rule as the worker
/// data report.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DashboardKpis {
    pub active_workers: i64,
    pub jobs_total: i64,
    pub jobs_today: i64,
    /// Jobs dated from today through the next 7 days (inclusive).
    pub jobs_next_7_days: i64,
    pub jobs_this_month: i64,
    pub jobs_last_month: i64,
    pub hours_this_month: f64,
    pub hours_last_month: f64,
    pub miles_this_month: f64,
    pub miles_last_month: f64,
    pub expenses_this_month_dollars: String,
    pub expenses_last_month_dollars: String,
    /// Workers signed in to a job today who haven't signed out yet.
    pub on_the_clock: i64,
}

/// One worker's assignment to one job.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DashboardAssignment {
    pub job_id: i64,
    pub worker_id: i64,
    pub worker_name: String,
    pub sitename: String,
    pub date: String,
    /// `HH:MM`, if signed in.
    pub signin: Option<String>,
    /// `HH:MM`, if signed out.
    pub signout: Option<String>,
}

/// A job and how many workers are assigned to it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DashboardJob {
    pub job_id: i64,
    pub sitename: String,
    pub address: String,
    pub date: String,
    pub workers_assigned: i64,
}

/// Things an admin probably needs to fix, over the last 60 days. The
/// `*_count` fields are the true totals; the lists are capped (most
/// recent first).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DashboardAttention {
    /// Jobs with nobody assigned.
    pub unstaffed_count: i64,
    pub unstaffed_jobs: Vec<DashboardJob>,
    /// Past assignments with a sign-in but no sign-out.
    pub missing_signout_count: i64,
    pub missing_signout: Vec<DashboardAssignment>,
    /// Past assignments with no sign-in at all.
    pub no_show_count: i64,
    pub no_shows: Vec<DashboardAssignment>,
}

/// One day of the activity chart.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DashboardDay {
    pub date: String,
    pub jobs: i64,
    pub hours: f64,
    /// `jobs` as a percentage (0-100) of the busiest day shown, for
    /// drawing bars.
    pub jobs_pct: u32,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DashboardWorker {
    pub worker_id: i64,
    pub name: String,
    pub hours: f64,
    pub jobs: i64,
    pub miles: f64,
}

/// Everything the admin homepage dashboard shows.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DashboardOutput {
    pub today: String,
    pub kpis: DashboardKpis,
    pub on_the_clock: Vec<DashboardAssignment>,
    /// Jobs from today through the next 7 days, soonest first.
    pub upcoming: Vec<DashboardJob>,
    pub attention: DashboardAttention,
    /// The last 30 days, oldest first.
    pub daily: Vec<DashboardDay>,
    /// The five workers with the most hours this month.
    pub top_workers: Vec<DashboardWorker>,
}

struct AsgRow {
    job_id: i64,
    worker_id: i64,
    worker_name: String,
    sitename: String,
    date: Date,
    signin: Option<Time>,
    signout: Option<Time>,
    miles: f64,
    expenses_cents: i64,
}

impl AsgRow {
    fn worked_hours(&self) -> Option<f64> {
        Some(hours_worked(self.signin?, self.signout?) as f64)
    }

    fn to_dto(&self) -> DashboardAssignment {
        let hm = |t: Time| format!("{:02}:{:02}", t.hour(), t.minute());
        DashboardAssignment {
            job_id: self.job_id,
            worker_id: self.worker_id,
            worker_name: self.worker_name.clone(),
            sitename: self.sitename.clone(),
            date: self.date.to_string(),
            signin: self.signin.map(hm),
            signout: self.signout.map(hm),
        }
    }
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn parse_time(s: Option<String>) -> Option<Time> {
    Time::parse(&s?, &Iso8601::TIME).ok()
}

async fn dashboard_core(
    pool: &Pool<Sqlite>,
    user: Option<&crate::CurrentUser>,
) -> Result<DashboardOutput, CustomError> {
    get_admin(user)?;

    let today = now().date();
    let month_start = today.replace_day(1).unwrap();
    let last_month_end = month_start - Duration::days(1);
    let last_month_start = last_month_end.replace_day(1).unwrap();
    let chart_start = today - Duration::days(29);
    let week_end = today + Duration::days(7);
    // Wide enough for last month, the 30-day chart and the 60-day
    // attention lookback; also covers the coming week.
    let window_start = last_month_start.min(today - Duration::days(60));

    let active_workers: i64 =
        sqlx::query("select count(*) from users where admin = false and deactivated = false")
            .fetch_one(pool)
            .await?
            .get(0);
    let jobs_total: i64 = sqlx::query("select count(*) from jobs")
        .fetch_one(pool)
        .await?
        .get(0);

    let jobs: Vec<DashboardJob> = sqlx::query(
        r#"
        select jobs.id, jobs.sitename, jobs.address, jobs.date,
            (select count(*) from jobworkers where jobworkers.job = jobs.id) as n
        from jobs
        where jobs.date >= $1 and jobs.date <= $2
        order by jobs.date asc, jobs.id asc
        "#,
    )
    .bind(window_start)
    .bind(week_end)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| DashboardJob {
        job_id: r.get("id"),
        sitename: r.get("sitename"),
        address: r.get("address"),
        date: r.get::<Date, _>("date").to_string(),
        workers_assigned: r.get("n"),
    })
    .collect();

    let asgs: Vec<AsgRow> = sqlx::query(
        r#"
        select jobworkers.job, jobworkers.worker, users.name, jobs.sitename, jobs.date,
            jobworkers.signin, jobworkers.signout, jobworkers.miles_driven,
            jobworkers.extraexpcents
        from jobworkers
        inner join jobs on jobs.id = jobworkers.job
        inner join users on users.id = jobworkers.worker
        where jobs.date >= $1 and jobs.date <= $2
        "#,
    )
    .bind(window_start)
    .bind(week_end)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| AsgRow {
        job_id: r.get("job"),
        worker_id: r.get("worker"),
        worker_name: r.get("name"),
        sitename: r.get("sitename"),
        date: r.get("date"),
        signin: parse_time(r.get("signin")),
        signout: parse_time(r.get("signout")),
        miles: r.get("miles_driven"),
        expenses_cents: r.get("extraexpcents"),
    })
    .collect();

    let date_of = |j: &DashboardJob| Date::parse(&j.date, &Iso8601::DATE).unwrap_or(today);
    let in_month = |d: Date| d >= month_start && d <= today;
    let in_last_month = |d: Date| d >= last_month_start && d <= last_month_end;

    // --- KPIs ---------------------------------------------------------
    let (mut hours_tm, mut hours_lm) = (0.0, 0.0);
    let (mut miles_tm, mut miles_lm) = (0.0, 0.0);
    let (mut exp_tm, mut exp_lm) = (0i64, 0i64);
    for a in asgs.iter().filter(|a| a.signin.is_some() && a.signout.is_some()) {
        let h = a.worked_hours().unwrap_or(0.0);
        if in_month(a.date) {
            hours_tm += h;
            miles_tm += a.miles;
            exp_tm += a.expenses_cents;
        } else if in_last_month(a.date) {
            hours_lm += h;
            miles_lm += a.miles;
            exp_lm += a.expenses_cents;
        }
    }
    let dollars = |c: i64| format!("{:.2}", c as f64 / 100.0);
    let on_the_clock_list: Vec<DashboardAssignment> = {
        let mut v: Vec<&AsgRow> = asgs
            .iter()
            .filter(|a| a.date == today && a.signin.is_some() && a.signout.is_none())
            .collect();
        v.sort_by_key(|a| a.signin);
        v.into_iter().map(AsgRow::to_dto).collect()
    };

    let kpis = DashboardKpis {
        active_workers,
        jobs_total,
        jobs_today: jobs.iter().filter(|j| date_of(j) == today).count() as i64,
        jobs_next_7_days: jobs
            .iter()
            .filter(|j| date_of(j) >= today && date_of(j) <= week_end)
            .count() as i64,
        jobs_this_month: jobs.iter().filter(|j| in_month(date_of(j))).count() as i64,
        jobs_last_month: jobs.iter().filter(|j| in_last_month(date_of(j))).count() as i64,
        hours_this_month: r2(hours_tm),
        hours_last_month: r2(hours_lm),
        miles_this_month: r2(miles_tm),
        miles_last_month: r2(miles_lm),
        expenses_this_month_dollars: dollars(exp_tm),
        expenses_last_month_dollars: dollars(exp_lm),
        on_the_clock: on_the_clock_list.len() as i64,
    };

    // --- Upcoming -----------------------------------------------------
    let upcoming: Vec<DashboardJob> = jobs
        .iter()
        .filter(|j| date_of(j) >= today)
        .take(DASH_LIST_LIMIT * 2)
        .cloned()
        .collect();

    // --- Needs attention (past 60 days, newest first) -----------------
    let lookback = today - Duration::days(60);
    let unstaffed_all: Vec<&DashboardJob> = jobs
        .iter()
        .rev()
        .filter(|j| j.workers_assigned == 0 && date_of(j) >= lookback)
        .collect();
    let mut past: Vec<&AsgRow> = asgs
        .iter()
        .filter(|a| a.date < today && a.date >= lookback)
        .collect();
    past.sort_by(|a, b| b.date.cmp(&a.date).then(a.worker_name.cmp(&b.worker_name)));
    let missing: Vec<&&AsgRow> = past
        .iter()
        .filter(|a| a.signin.is_some() && a.signout.is_none())
        .collect();
    let no_shows: Vec<&&AsgRow> = past.iter().filter(|a| a.signin.is_none()).collect();

    let attention = DashboardAttention {
        unstaffed_count: unstaffed_all.len() as i64,
        unstaffed_jobs: unstaffed_all
            .iter()
            .take(DASH_LIST_LIMIT)
            .map(|j| (*j).clone())
            .collect(),
        missing_signout_count: missing.len() as i64,
        missing_signout: missing.iter().take(DASH_LIST_LIMIT).map(|a| a.to_dto()).collect(),
        no_show_count: no_shows.len() as i64,
        no_shows: no_shows.iter().take(DASH_LIST_LIMIT).map(|a| a.to_dto()).collect(),
    };

    // --- 30-day chart -------------------------------------------------
    let mut per_day: HashMap<Date, (i64, f64)> = HashMap::new();
    for j in jobs.iter().filter(|j| date_of(j) >= chart_start && date_of(j) <= today) {
        per_day.entry(date_of(j)).or_default().0 += 1;
    }
    for a in asgs.iter().filter(|a| a.date >= chart_start && a.date <= today) {
        if let Some(h) = a.worked_hours() {
            per_day.entry(a.date).or_default().1 += h;
        }
    }
    let max_jobs = per_day.values().map(|v| v.0).max().unwrap_or(0).max(1);
    let daily = (0..30)
        .map(|i| {
            let d = chart_start + Duration::days(i);
            let (jobs, hours) = per_day.get(&d).copied().unwrap_or_default();
            DashboardDay {
                date: d.to_string(),
                jobs,
                hours: r2(hours),
                jobs_pct: (jobs * 100 / max_jobs) as u32,
            }
        })
        .collect();

    // --- Top workers this month ---------------------------------------
    let mut by_worker: HashMap<i64, DashboardWorker> = HashMap::new();
    for a in asgs
        .iter()
        .filter(|a| in_month(a.date) && a.signin.is_some() && a.signout.is_some())
    {
        let w = by_worker.entry(a.worker_id).or_insert_with(|| DashboardWorker {
            worker_id: a.worker_id,
            name: a.worker_name.clone(),
            hours: 0.0,
            jobs: 0,
            miles: 0.0,
        });
        w.hours += a.worked_hours().unwrap_or(0.0);
        w.jobs += 1;
        w.miles += a.miles;
    }
    let mut top_workers: Vec<DashboardWorker> = by_worker.into_values().collect();
    top_workers.sort_by(|a, b| b.hours.total_cmp(&a.hours).then(a.name.cmp(&b.name)));
    top_workers.truncate(5);
    for w in &mut top_workers {
        w.hours = r2(w.hours);
        w.miles = r2(w.miles);
    }

    Ok(DashboardOutput {
        today: today.to_string(),
        kpis,
        on_the_clock: on_the_clock_list,
        upcoming,
        attention,
        daily,
        top_workers,
    })
}

/// A detailed operations dashboard for admins: headline numbers, who is
/// on the clock today, upcoming jobs, things needing attention (unstaffed
/// jobs, missing sign-outs, no-shows), a 30-day activity chart and this
/// month's busiest workers.
#[utoipa::path(
    get,
    path = "/admin/api/v1/dashboard",
    responses((status = OK, body = DashboardOutput)),
    security(("bearer_auth" = [])),
    tag = super::ADMIN_TAG
)]
pub(crate) async fn dashboard_api(
    State(AppState { pool, .. }): State<AppState>,
    ApiAuth { user, .. }: ApiAuth,
) -> Result<Json<DashboardOutput>, ApiError> {
    to_api(dashboard_core(&pool, Some(&user)).await)
}
