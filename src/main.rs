use academia_dl::download::{download_to_file, download_url};
use academia_dl::fetch::{build_client, fetch_page_html};
use academia_dl::filename::filename_from_url;
use academia_dl::parse::extract_download_id;
use academia_dl::validate::validate_academia_url;
use anyhow::Result;
use clap::Parser;
use tokio::task::JoinSet;

#[derive(Parser)]
#[command(
    name = "academia-dl",
    version,
    about = "Download PDFs from academia.edu and scribd.com without logging in"
)]
struct Args {
    /// Cookie header value from a logged-in browser session
    /// (e.g. DevTools → Application → Cookies → copy cf_clearance etc. as "a=b; c=d").
    /// Also read from ACADEMIA_COOKIES env var.
    #[arg(long, env = "ACADEMIA_COOKIES")]
    cookie: Option<String>,

    /// One or more academia.edu / scribd.com URLs to download
    #[arg(required = true)]
    urls: Vec<String>,
}

/// Report a failure line: via the progress UI on a TTY, via stderr when
/// output is piped (indicatif swallows `mp.println` when hidden).
fn report(mp: &indicatif::MultiProgress, msg: String) {
    if mp.is_hidden() {
        eprintln!("{msg}");
    } else {
        let _ = mp.println(msg);
    }
}

fn host_of(input: &str) -> Option<String> {
    url::Url::parse(input)
        .ok()?
        .host_str()
        .map(|h| h.to_lowercase())
}

async fn process_one(
    client: &wreq::Client,
    mp: &indicatif::MultiProgress,
    input: &str,
) -> Result<()> {
    match host_of(input) {
        Some(h) if h == "scribd.com" || h.ends_with(".scribd.com") => {
            process_scribd(client, mp, input).await
        }
        _ => process_academia(client, mp, input).await,
    }
}

async fn process_scribd(
    client: &wreq::Client,
    mp: &indicatif::MultiProgress,
    input: &str,
) -> Result<()> {
    let url = academia_dl::validate::validate_scribd_url(input)?;
    let filename = filename_from_url(&url);
    if tokio::fs::try_exists(&filename).await.unwrap_or(false) {
        let _ = mp.println(format!("{filename} already exists, skipping"));
        return Ok(());
    }
    let html = fetch_page_html(client, &url).await?;
    academia_dl::scribd::download_scribd_pdf(client, Some(mp), None, &html, &filename).await?;
    Ok(())
}

async fn process_academia(
    client: &wreq::Client,
    mp: &indicatif::MultiProgress,
    input: &str,
) -> Result<()> {
    let url = validate_academia_url(input)?;
    let filename = filename_from_url(&url);
    if tokio::fs::try_exists(&filename).await.unwrap_or(false) {
        let _ = mp.println(format!("{filename} already exists, skipping"));
        return Ok(());
    }
    let html = fetch_page_html(client, &url).await?;
    if let Some(direct) = academia_dl::parse::extract_download_url(&html) {
        download_to_file(client, Some(mp), None, &direct, &filename).await?;
        return Ok(());
    }
    let download_id = extract_download_id(&html)?;
    let dl_url = download_url(&download_id, &filename);
    download_to_file(client, Some(mp), None, &dl_url, &filename).await?;
    Ok(())
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let client = build_client(args.cookie.as_deref()).unwrap_or_else(|e| {
        eprintln!("{e:?}");
        std::process::exit(1);
    });
    let mut set = JoinSet::new();
    let mp = indicatif::MultiProgress::new();
    let mut seen = std::collections::HashSet::new();
    let urls: Vec<String> = args
        .urls
        .into_iter()
        .filter(|u| seen.insert(u.clone()))
        .collect();
    for input in urls {
        let client = client.clone();
        let mp = mp.clone();
        set.spawn(async move { (input.clone(), process_one(&client, &mp, &input).await) });
    }
    let mut failed = false;
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok((_, Ok(()))) => {}
            Ok((input, Err(e))) => {
                failed = true;
                report(
                    &mp,
                    format!("Error parsing/downloading file for URL {input}: {e:?}"),
                );
            }
            Err(e) => {
                failed = true;
                report(&mp, format!("task failed: {e:?}"));
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}
