use std::time::Duration;

use anyhow::{Context, Result};
use url::Url;
use wreq::header::{HeaderMap, HeaderValue, ACCEPT_LANGUAGE, COOKIE, REFERER};
use wreq_util::{Emulation, Platform, Profile};

pub const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
pub const REFERER_VALUE: &str = "http://scholar.google.com";
pub const MAX_RETRIES: u32 = 5;
pub const RETRY_SLEEP_SECS: u64 = 5;

pub fn default_headers(cookies: Option<&str>) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        wreq::header::USER_AGENT,
        HeaderValue::from_static(USER_AGENT),
    );
    headers.insert(REFERER, HeaderValue::from_static(REFERER_VALUE));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    if let Some(c) = cookies {
        let c = c.trim();
        if !c.is_empty() {
            headers.insert(
                COOKIE,
                HeaderValue::from_str(c).context("invalid cookie string")?,
            );
        }
    }
    Ok(headers)
}

pub fn build_client(cookies: Option<&str>) -> Result<wreq::Client> {
    // NOTE: emulation overwrites TLS/HTTP2/headers config, so it must come
    // FIRST; default_headers() after ensures our UA/Referer/Cookie survive.
    let client = wreq::Client::builder()
        .emulation(
            Emulation::builder()
                .profile(Profile::Chrome149)
                .platform(Platform::Linux)
                .build(),
        )
        .default_headers(default_headers(cookies)?)
        .redirect(wreq::redirect::Policy::limited(10))
        // Total timeout would kill slow large downloads mid-stream;
        // bound only the connect phase instead.
        .connect_timeout(Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")?;
    Ok(client)
}

pub async fn fetch_page_html(client: &wreq::Client, url: &Url) -> Result<String> {
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        let result = client.get(url.as_str()).send().await;
        let failed = match result {
            Ok(resp) => match resp.error_for_status() {
                Ok(ok) => return ok.text().await.context("failed to read page body"),
                Err(e) => {
                    eprintln!("{e:?}");
                    true
                }
            },
            Err(e) => {
                eprintln!("{e:?}");
                true
            }
        };
        if failed && attempt >= MAX_RETRIES {
            eprintln!("Hint: HTTP 403 from academia.edu usually means bot protection blocked the request. Try a browser-like network or update headers in src/fetch.rs.");
            anyhow::bail!(
                "Max retries (= {MAX_RETRIES}) reached, exiting after trying to open URL: {url}"
            );
        }
        tokio::time::sleep(Duration::from_secs(RETRY_SLEEP_SECS)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_has_browser_headers() {
        // NOTE: wreq merges `default_headers` at `execute()` time, not in
        // `RequestBuilder::build()`, so `client.get(..).build()` never shows
        // them. Assert on the map the client is built with instead (pure, no network).
        let headers = default_headers(None).unwrap();
        assert!(headers
            .get(wreq::header::USER_AGENT)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("Chrome"));
        assert_eq!(
            headers.get(wreq::header::REFERER).unwrap(),
            "http://scholar.google.com"
        );
        assert!(headers.get(COOKIE).is_none());
        // Client must still build successfully with those defaults.
        build_client(None).unwrap();
    }

    #[test]
    fn cookie_header_set_when_provided() {
        let headers = default_headers(Some("cf_clearance=abc123; __cf_bm=xyz")).unwrap();
        assert_eq!(
            headers.get(COOKIE).unwrap(),
            "cf_clearance=abc123; __cf_bm=xyz"
        );
        // Empty/whitespace cookies must not set the header.
        assert!(default_headers(Some("   ")).unwrap().get(COOKIE).is_none());
        // Client must still build successfully with cookies.
        build_client(Some("cf_clearance=abc123; __cf_bm=xyz")).unwrap();
    }
}
