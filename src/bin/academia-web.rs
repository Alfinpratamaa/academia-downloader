use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use academia_dl::download::{download_to_file, download_url, ProgressUpdate};
use academia_dl::fetch::{build_client, fetch_page_html};
use academia_dl::filename::filename_from_url;
use academia_dl::parse::{extract_download_id, extract_download_url};
use academia_dl::validate::{validate_academia_url, validate_scribd_url};
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Form;
use clap::Parser;
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex};

const MAX_ACTIVE_JOBS: usize = 3;

#[derive(Parser)]
#[command(name = "academia-web", version, about = "Web UI for academia-dl")]
struct Args {
    #[arg(long, default_value = "3000")]
    port: u16,

    #[arg(long, default_value = "./downloads")]
    dir: PathBuf,

    #[arg(long, default_value = "127.0.0.1")]
    bind: String,

    /// Cookie header value from a logged-in browser session.
    /// Also read from ACADEMIA_COOKIES env var.
    #[arg(long, env = "ACADEMIA_COOKIES")]
    cookie: Option<String>,
}

struct JobState {
    latest: Option<ProgressUpdate>,
    done: bool,
    error: Option<String>,
}

struct Job {
    filename: String,
    path: PathBuf,
    state: Arc<Mutex<JobState>>,
}

struct AppState {
    jobs: Mutex<HashMap<String, Job>>,
    counter: AtomicU64,
    client: wreq::Client,
    dir: PathBuf,
}

#[derive(Deserialize)]
struct JobForm {
    url: String,
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

fn err_fragment(msg: &str) -> String {
    format!("<p class=\"err\">{}</p>", esc(msg))
}

fn poll_div(id: &str, inner: &str) -> String {
    format!(
        "<div hx-get=\"/jobs/{}\" hx-trigger=\"every 500ms\" hx-swap=\"outerHTML\">{}</div>",
        esc(id),
        inner
    )
}

fn progress_inner(filename: &str, st: &JobState) -> String {
    match &st.latest {
        None => format!("<p>queued&hellip; {}</p>", esc(filename)),
        Some(u) => match u.total {
            Some(t) if t > 0 => {
                let pct = (u.downloaded.saturating_mul(100) / t).min(100);
                format!(
                    "<p>{} &mdash; {} / {} bytes ({}%)</p>\
                     <div class=\"bar\"><div class=\"fill\" style=\"width: {}%\"></div></div>",
                    esc(filename),
                    u.downloaded,
                    t,
                    pct,
                    pct
                )
            }
            _ => format!(
                "<p>{} &mdash; {} bytes (total unknown)&hellip;</p>\
                 <div class=\"bar indeterminate\"></div>",
                esc(filename),
                u.downloaded
            ),
        },
    }
}

async fn security_headers(req: axum::http::Request<axum::body::Body>, next: Next) -> Response {
    let mut resp = next.run(req).await;
    resp.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        header::HeaderValue::from_static(
            "default-src 'self'; script-src 'self' https://unpkg.com; style-src 'self' 'unsafe-inline'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
        ),
    );
    resp.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        header::HeaderValue::from_static("nosniff"),
    );
    resp
}

async fn index() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("../../web/index.html"),
    )
}

async fn active_count(state: &Arc<AppState>) -> usize {
    let jobs = state.jobs.lock().await;
    let mut n = 0;
    for job in jobs.values() {
        let st = job.state.lock().await;
        if !st.done && st.error.is_none() {
            n += 1;
        }
    }
    n
}

async fn create_job(
    State(state): State<Arc<AppState>>,
    Form(form): Form<JobForm>,
) -> Result<impl IntoResponse, (StatusCode, Html<String>)> {
    let host = url::Url::parse(&form.url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
        .unwrap_or_default();
    let url = if host == "scribd.com" || host.ends_with(".scribd.com") {
        validate_scribd_url(&form.url)
    } else {
        validate_academia_url(&form.url)
    }
    .map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Html(err_fragment(&format!("{e:?}"))),
        )
    })?;
    if active_count(&state).await >= MAX_ACTIVE_JOBS {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Html(err_fragment(
                "too many active jobs (max 3), try again later",
            )),
        ));
    }
    let filename = filename_from_url(&url);
    let id = state.counter.fetch_add(1, Ordering::SeqCst).to_string();
    let path = state.dir.join(&filename);
    let job = Job {
        filename: filename.clone(),
        path: path.clone(),
        state: Arc::new(Mutex::new(JobState {
            latest: None,
            done: false,
            error: None,
        })),
    };
    state.jobs.lock().await.insert(id.clone(), job);
    let worker_state = state.clone();
    let worker_id = id.clone();
    let path_str = path.to_string_lossy().into_owned();
    tokio::spawn(async move {
        let (tx, mut rx) = mpsc::channel::<ProgressUpdate>(16);
        let st = {
            let jobs = worker_state.jobs.lock().await;
            jobs.get(&worker_id).map(|j| j.state.clone())
        };
        let Some(st) = st else { return };
        let st_recv = st.clone();
        let recv = tokio::spawn(async move {
            while let Some(u) = rx.recv().await {
                st_recv.lock().await.latest = Some(u);
            }
        });
        let result = run_job(&worker_state.client, &url, &path_str, tx).await;
        // Channel closed (tx dropped with run_job) → receiver drains then ends.
        let _ = recv.await;
        let mut guard = st.lock().await;
        match result {
            Ok(()) => guard.done = true,
            Err(e) => guard.error = Some(format!("{e:?}")),
        }
        eprintln!(
            "job {worker_id} finished: done={} err={:?}",
            guard.done, guard.error
        );
    });
    Ok(Html(poll_div(&id, "queued&hellip;")))
}

async fn run_job(
    client: &wreq::Client,
    url: &url::Url,
    path: &str,
    tx: mpsc::Sender<ProgressUpdate>,
) -> anyhow::Result<()> {
    eprintln!("job starting: {url} -> {path}");
    let host = url.host_str().unwrap_or("").to_lowercase();
    if host == "scribd.com" || host.ends_with(".scribd.com") {
        let html = fetch_page_html(client, url).await?;
        academia_dl::scribd::download_scribd_pdf(client, None, Some(tx), &html, path).await?;
        return Ok(());
    }
    let html = fetch_page_html(client, url).await?;
    if let Some(direct) = extract_download_url(&html) {
        download_to_file(client, None, Some(tx), &direct, path).await?;
        return Ok(());
    }
    let download_id = extract_download_id(&html)?;
    let filename = std::path::Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download.pdf".to_string());
    let dl_url = download_url(&download_id, &filename);
    download_to_file(client, None, Some(tx), &dl_url, path).await?;
    Ok(())
}

async fn job_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Html<String>)> {
    let (filename, st_snapshot) = {
        let jobs = state.jobs.lock().await;
        let Some(job) = jobs.get(&id) else {
            return Err((StatusCode::NOT_FOUND, Html(err_fragment("unknown job"))));
        };
        let st = job.state.lock().await;
        (
            job.filename.clone(),
            JobState {
                latest: st.latest.clone(),
                done: st.done,
                error: st.error.clone(),
            },
        )
    };
    if let Some(err) = st_snapshot.error {
        return Ok(Html(err_fragment(&err)));
    }
    if st_snapshot.done {
        return Ok(Html(format!(
            "<p>Done: {}</p><a href=\"/files/{}\">Download {}</a>",
            esc(&filename),
            esc(&id),
            esc(&filename)
        )));
    }
    Ok(Html(poll_div(
        &id,
        &progress_inner(&filename, &st_snapshot),
    )))
}

async fn serve_file(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let (filename, path, done) = {
        let jobs = state.jobs.lock().await;
        match jobs.get(&id) {
            Some(job) => {
                let st = job.state.lock().await;
                (
                    job.filename.clone(),
                    job.path.clone(),
                    st.done && st.error.is_none(),
                )
            }
            None => {
                return (StatusCode::NOT_FOUND, Html(err_fragment("unknown job"))).into_response();
            }
        }
    };
    if !done {
        return (StatusCode::NOT_FOUND, Html(err_fragment("file not ready"))).into_response();
    }
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let safe_name = filename.replace(['"', '\r', '\n'], "_");
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "application/pdf".to_string()),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{safe_name}\""),
                    ),
                ],
                bytes,
            )
                .into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, Html(err_fragment("file not found"))).into_response(),
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    if let Err(e) = tokio::fs::create_dir_all(&args.dir).await {
        eprintln!(
            "failed to create download dir {}: {e:?}",
            args.dir.display()
        );
        std::process::exit(1);
    }
    // Canonicalize so stored job paths are absolute; users never supply paths.
    let dir = args.dir.canonicalize().unwrap_or_else(|_| args.dir.clone());
    let client = build_client(args.cookie.as_deref()).unwrap_or_else(|e| {
        eprintln!("{e:?}");
        std::process::exit(1);
    });
    let state = Arc::new(AppState {
        jobs: Mutex::new(HashMap::new()),
        counter: AtomicU64::new(1),
        client,
        dir,
    });
    let app = axum::Router::new()
        .route("/", get(index))
        .route("/jobs", post(create_job))
        .route("/jobs/{id}", get(job_status))
        .route("/files/{id}", get(serve_file))
        .layer(middleware::from_fn(security_headers))
        .with_state(state);
    let addr: SocketAddr = format!("{}:{}", args.bind, args.port)
        .parse()
        .unwrap_or_else(|e| {
            eprintln!("invalid bind address: {e:?}");
            std::process::exit(1);
        });
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("failed to bind {addr}: {e:?}");
            std::process::exit(1);
        });
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("server error: {e:?}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html_specials() {
        assert_eq!(esc("&<>\"'"), "&amp;&lt;&gt;&quot;&#x27;");
        assert_eq!(esc("plain.pdf"), "plain.pdf");
        assert_eq!(
            esc("<script>alert('x')</script>"),
            "&lt;script&gt;alert(&#x27;x&#x27;)&lt;/script&gt;"
        );
    }

    #[test]
    fn err_fragment_escapes() {
        let out = err_fragment("<b>&\"</b>");
        assert_eq!(out, "<p class=\"err\">&lt;b&gt;&amp;&quot;&lt;/b&gt;</p>");
    }

    #[test]
    fn poll_div_targets_job() {
        let out = poll_div("42", "queued&hellip;");
        assert!(out.contains("hx-get=\"/jobs/42\""));
        assert!(out.contains("every 500ms"));
        assert!(out.contains("outerHTML"));
    }
}
