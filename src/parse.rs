use anyhow::{bail, Result};
use regex::Regex;
use scraper::{Html, Selector};

fn debug_candidates(text: &str, primary: &Regex) {
    if std::env::var("ACADEMIA_DEBUG").is_ok() {
        for caps in primary.captures_iter(text) {
            let m = caps.get(0).unwrap();
            let rest = &text[m.end()..];
            let next_entity = rest.find("[{\"id\":").unwrap_or(rest.len());
            let ctx_end = next_entity.min(300);
            let ctx = &rest[..ctx_end];
            let keys: Vec<&str> = ctx
                .split(['"', ':', '{', '}', ',', '[', ']'])
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.len() < 40)
                .take(12)
                .collect();
            eprintln!("[debug] candidate id={} keys={keys:?}", &caps[1]);
        }
    }
}
fn debug_branch(branch: &str, id: &str, context: &str) {
    if std::env::var("ACADEMIA_DEBUG").is_ok() {
        let snippet: String = context.chars().take(160).collect();
        eprintln!("[debug] matched {branch}: id={id} context={snippet}");
    }
}

/// Direct file URL embedded in the page (logged-in variant), e.g.
/// `"downloadUrl":"https://www.academia.edu/attachments/61934898/download_file"`.
/// Preferred over id-based URL construction when present.
pub fn extract_download_url(html: &str) -> Option<String> {
    let re = Regex::new(r#""downloadUrl":"([^"]{0,300})"#).ok()?;
    re.captures(html)
        .map(|c| c[1].replace("\\/", "/").replace("\\\\", "\\").to_string())
}

pub fn extract_download_id(html: &str) -> Result<String> {
    if std::env::var("ACADEMIA_DEBUG").is_ok() {
        eprintln!("[debug] html len={}", html.len());
        for pat in [
            "\"identifier\"",
            "shouldShowBulkDownload",
            "auto=download",
            "\"downloadUrl\"",
            "\"pdfUrl\"",
            "attachment_thumbnails",
        ] {
            let n = html.matches(pat).count();
            eprintln!("[debug] pattern {pat:?} count={n}");
        }
        let re_dl = Regex::new(r"/download/(\d+)").unwrap();
        let ids: Vec<&str> = re_dl
            .captures_iter(html)
            .take(5)
            .map(|c| c.get(1).unwrap().as_str())
            .collect();
        eprintln!("[debug] /download/ ids={ids:?}");
        let re_du = Regex::new(r#""downloadUrl":"([^"]{0,300})"#).unwrap();
        let dus: Vec<&str> = re_du
            .captures_iter(html)
            .take(3)
            .map(|c| c.get(1).unwrap().as_str())
            .collect();
        eprintln!("[debug] downloadUrl={dus:?}");
    }
    let primary = Regex::new(r#"\[\{"id":(\d+)"#).unwrap();
    let document = Html::parse_document(html);
    let selector = Selector::parse("script").unwrap();
    let mut first_primary: Option<String> = None;
    for script in document.select(&selector) {
        let text: String = script.text().collect();
        debug_candidates(&text, &primary);
        for caps in primary.captures_iter(&text) {
            let id = caps[1].to_string();
            if first_primary.is_none() {
                first_primary = Some(id.clone());
            }
            // The download entity is followed by "identifier" (author and
            // other entities share the [{"id": shape but lack it).
            // Bound the window at the next entity so a later "identifier"
            // cannot leak into this match's context.
            let m = caps.get(0).unwrap();
            let rest = &text[m.end()..];
            let next_entity = rest.find("[{\"id\":").unwrap_or(rest.len());
            let ctx_end = next_entity.min(600);
            if rest[..ctx_end].contains("\"identifier\"") {
                debug_branch("script-tag-identifier", &id, &text);
                return Ok(id);
            }
        }
    }
    if let Some(id) = first_primary {
        debug_branch("script-tag-first", &id, "");
        return Ok(id);
    }
    if let Some(caps) = primary.captures(html) {
        let m = caps.get(0).unwrap();
        let start = m.start().saturating_sub(80);
        debug_branch(
            "raw-html-primary",
            &caps[1],
            &html[start..m.end().min(start + 160)],
        );
        return Ok(caps[1].to_string());
    }
    let meta_selector = Selector::parse(r#"meta[property="og:url"]"#).unwrap();
    let meta_id_inner = Regex::new(r"academia\.edu/(\d+)").unwrap();
    for meta in document.select(&meta_selector) {
        if let Some(content) = meta.value().attr("content") {
            if let Some(caps) = meta_id_inner.captures(content) {
                debug_branch("og:url", &caps[1], content);
                return Ok(caps[1].to_string());
            }
        }
    }
    let meta_id = Regex::new(r#"academia\.edu/(\d+)"#).unwrap();
    if let Some(caps) = meta_id.captures(html) {
        let m = caps.get(0).unwrap();
        let start = m.start().saturating_sub(80);
        debug_branch(
            "broad-fallback",
            &caps[1],
            &html[start..m.end().min(start + 160)],
        );
        return Ok(caps[1].to_string());
    }
    bail!("could not find download id in page HTML")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: &str =
        r#"<html><body><script>window.foo = [{"id":41779665,"foo":1}];</script></body></html>"#;
    const FALLBACK_META: &str = r#"<html><head><meta property="og:url" content="https://www.academia.edu/99887766/Some_Paper" /></head><body></body></html>"#;
    const NONE: &str = "<html><body><p>no id here</p></body></html>";

    #[test]
    fn extracts_primary_script_id() {
        assert_eq!(extract_download_id(PRIMARY).unwrap(), "41779665");
    }

    #[test]
    fn falls_back_to_og_url() {
        assert_eq!(extract_download_id(FALLBACK_META).unwrap(), "99887766");
    }

    #[test]
    fn errors_when_no_id() {
        assert!(extract_download_id(NONE).is_err());
    }

    const OG_PRECEDENCE: &str = r#"<html><head><meta property="og:url" content="https://www.academia.edu/222/Some_Paper" /></head><body><a href="https://www.academia.edu/111/other">x</a></body></html>"#;

    #[test]
    fn prefers_og_url_over_body_links() {
        assert_eq!(extract_download_id(OG_PRECEDENCE).unwrap(), "222");
    }

    const AUTHOR_FIRST: &str = r#"<html><body><script>[{"id":143929526,"displayName":"X","bio":"y"}];[{"id":61934898,"identifier":"w","shouldShowBulkDownload":false}];</script></body></html>"#;

    #[test]
    fn prefers_id_followed_by_identifier() {
        assert_eq!(extract_download_id(AUTHOR_FIRST).unwrap(), "61934898");
    }

    const NO_IDENTIFIER: &str =
        r#"<html><body><script>[{"id":111,"foo":1}];</script></body></html>"#;

    #[test]
    fn falls_back_to_first_without_identifier() {
        assert_eq!(extract_download_id(NO_IDENTIFIER).unwrap(), "111");
    }

    const DIRECT: &str = r#"<html><body><script>{"props":{"downloadUrl":"https://www.academia.edu/attachments/61934898/download_file"}}</script></body></html>"#;

    #[test]
    fn extracts_direct_download_url() {
        assert_eq!(
            extract_download_url(DIRECT).unwrap(),
            "https://www.academia.edu/attachments/61934898/download_file"
        );
    }

    #[test]
    fn no_direct_url_returns_none() {
        assert!(extract_download_url(PRIMARY).is_none());
    }
}
