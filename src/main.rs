mod download;
mod fetch;
mod filename;
mod parse;
mod validate;

use anyhow::Result;
use clap::Parser;
use tokio::task::JoinSet;

use download::{download_to_file, download_url};
use fetch::{build_client, fetch_page_html};
use filename::filename_from_url;
use parse::extract_download_id;
use validate::validate_academia_url;

#[derive(Parser)]
#[command(
    name = "academia-dl",
    version,
    about = "Download PDFs from academia.edu without logging in"
)]
struct Args {
    /// Cookie header value from a logged-in browser session
    /// (e.g. DevTools → Application → Cookies → copy cf_clearance etc. as "a=b; c=d").
    /// Also read from ACADEMIA_COOKIES env var.
    #[arg(long, env = "ACADEMIA_COOKIES")]
    cookie: Option<String>,

    /// One or more academia.edu URLs to download
    #[arg(required = true)]
    urls: Vec<String>,
}

async fn process_one(
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
    if let Some(direct) = parse::extract_download_url(&html) {
        download_to_file(client, mp, &direct, &filename).await?;
        return Ok(());
    }
    let download_id = extract_download_id(&html)?;
    let dl_url = download_url(&download_id, &filename);
    download_to_file(client, mp, &dl_url, &filename).await?;
    Ok(())
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    if args
        .cookie
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
    {
        eprintln!("No cookies provided (--cookie or ACADEMIA_COOKIES); anonymous requests will likely get HTTP 403 from Cloudflare.");
    }
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
                let _ = mp.println(format!(
                    "Error parsing/downloading file for URL {input}: {e:?}"
                ));
            }
            Err(e) => {
                failed = true;
                let _ = mp.println(format!("task failed: {e:?}"));
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}
