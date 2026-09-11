//! Scribd download path (no auth).
//!
//! A public scribd.com document page embeds one JSONP URL per page:
//! `contentUrl: "https://html.scribdassets.com/<hash>/pages/<n>-<token>.jsonp"`.
//! Each JSONP payload contains the rendered page image via
//! `orig=\"http://html.scribd.com/<hash>/images/<n>-<token>.jpg\"`.
//! All pages (including the last) are reachable anonymously, so the PDF is
//! reconstructed by downloading every page JPEG and embedding each as a
//! DCTDecode image (one page per image) with a minimal pure-std PDF writer.

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::sync::Arc;

use crate::download::ProgressUpdate;

/// Max concurrent page (JSONP / image) fetches per document.
pub const PAGE_CONCURRENCY: usize = 8;

/// One downloaded page ready for PDF embedding.
pub struct PdfPage {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// JPEG component count: 1 = gray, 3 = RGB.
    pub components: u8,
}

/// Extract per-page JSONP URLs from document HTML, ordered by page number.
///
/// Matches `contentUrl: "https://.../pages/<n>-<token>.jsonp"` and sorts by `<n>`.
pub fn extract_page_jsonp_urls(html: &str) -> Result<Vec<String>> {
    let re = Regex::new(r#"contentUrl:\s*"(https://[^"]+/pages/(\d+)-[^"]+\.jsonp)""#)
        .map_err(|e| anyhow::anyhow!("invalid regex: {e}"))?;
    let mut pages: Vec<(u32, String)> = re
        .captures_iter(html)
        .filter_map(|c| {
            let n: u32 = c.get(2)?.as_str().parse().ok()?;
            Some((n, c.get(1)?.as_str().to_string()))
        })
        .collect();
    if pages.is_empty() {
        if html.contains("Client Challenge") || html.contains("_fs-ch") {
            bail!(
                "scribd served a bot-challenge page instead of the document (this IP is flagged). \
                Open the URL in a real browser, then re-run with cookies from that session: \
                academia-dl --cookie \"$(...)\" <url> (or export ACADEMIA_COOKIES)"
            );
        }
        bail!("could not find scribd page JSONP URLs in page HTML");
    }
    pages.sort_by_key(|(n, _)| *n);
    pages.dedup_by_key(|(n, _)| *n);
    Ok(pages.into_iter().map(|(_, u)| u).collect())
}

/// Extract the page image URL from a JSONP payload.
///
/// Handles the JS-escaped form (`orig=\"...jpg\"`) and the plain form
/// (`orig="...jpg"`). Upgrades `http://` to `https://`.
pub fn extract_page_image_url(jsonp: &str) -> Result<String> {
    for pat in [
        r#"orig=\\"(https?://[^\\"]+?\.jpg)\\""#,
        r#"orig="(https?://[^"]+?\.jpg)""#,
    ] {
        let re = Regex::new(pat).map_err(|e| anyhow::anyhow!("invalid regex: {e}"))?;
        if let Some(caps) = re.captures(jsonp) {
            let mut url = caps[1].to_string();
            if let Some(rest) = url.strip_prefix("http://") {
                url = format!("https://{rest}");
            }
            return Ok(url);
        }
    }
    bail!("could not find page image URL in scribd JSONP payload")
}

/// Read `(width, height, components)` from a JPEG's SOF marker (pure std).
pub fn jpeg_dimensions(jpeg: &[u8]) -> Result<(u32, u32, u8)> {
    if jpeg.len() < 4 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        bail!("not a JPEG (missing SOI marker)");
    }
    let mut i = 2;
    while i + 4 <= jpeg.len() {
        if jpeg[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = jpeg[i + 1];
        // Standalone markers without a length field.
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            i += 2;
            continue;
        }
        if i + 4 > jpeg.len() {
            break;
        }
        let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
        if len < 2 || i + 2 + len > jpeg.len() {
            bail!("truncated JPEG segment");
        }
        // SOF0-SOF3, SOF5-SOF7, SOF9-SOF11, SOF13-SOF15 (skip DHT/DAC/JPG).
        if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
            if len < 8 {
                bail!("truncated JPEG SOF segment");
            }
            let h = u16::from_be_bytes([jpeg[i + 5], jpeg[i + 6]]) as u32;
            let w = u16::from_be_bytes([jpeg[i + 7], jpeg[i + 8]]) as u32;
            let comp = jpeg[i + 9];
            if w == 0 || h == 0 {
                bail!("invalid JPEG dimensions");
            }
            return Ok((w, h, comp));
        }
        if marker == 0xDA {
            bail!("reached SOS without finding SOF marker");
        }
        i += 2 + len;
    }
    bail!("could not find SOF marker in JPEG")
}

/// Assemble one PDF page per JPEG (DCTDecode embedding, 1px = 1pt).
pub fn build_pdf(pages: &[PdfPage]) -> Result<Vec<u8>> {
    if pages.is_empty() {
        bail!("no pages to write");
    }
    // Object numbering: 1 = catalog, 2 = /Pages, then per page
    // (page, content stream, image XObject).
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n");
    let mut offsets: Vec<usize> = vec![0];

    let push_obj = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, n: usize, body: &[u8]| {
        offsets.push(out.len());
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };

    push_obj(
        &mut out,
        &mut offsets,
        1,
        b"<< /Type /Catalog /Pages 2 0 R >>",
    );

    let kids: String = pages
        .iter()
        .enumerate()
        .map(|(i, _)| format!("{} 0 R ", 3 + i * 3))
        .collect();
    push_obj(
        &mut out,
        &mut offsets,
        2,
        format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()).as_bytes(),
    );

    for (i, page) in pages.iter().enumerate() {
        let page_obj = 3 + i * 3;
        let content_obj = page_obj + 1;
        let img_obj = page_obj + 2;
        let colorspace = match page.components {
            1 => "/DeviceGray",
            _ => "/DeviceRGB",
        };
        push_obj(
            &mut out,
            &mut offsets,
            page_obj,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] \
                 /Resources << /XObject << /Im{i} {img_obj} 0 R >> >> \
                 /Contents {content_obj} 0 R >>",
                page.width, page.height
            )
            .as_bytes(),
        );
        let content = format!("q\n{} 0 0 {} 0 0 cm\n/Im{i} Do\nQ", page.width, page.height);
        push_obj(
            &mut out,
            &mut offsets,
            content_obj,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            )
            .as_bytes(),
        );
        let mut img = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} \
             /ColorSpace {colorspace} /BitsPerComponent 8 \
             /Filter /DCTDecode /Length {} >>\nstream\n",
            page.width,
            page.height,
            page.jpeg.len()
        )
        .into_bytes();
        img.extend_from_slice(&page.jpeg);
        img.extend_from_slice(b"\nendstream");
        push_obj(&mut out, &mut offsets, img_obj, &img);
    }

    let xref_pos = out.len();
    let count = offsets.len();
    out.extend_from_slice(format!("xref\n0 {count}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for off in offsets.iter().skip(1) {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {count} /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF")
            .as_bytes(),
    );
    Ok(out)
}

/// Fetch one URL body as bytes with error context.
pub async fn fetch_bytes(client: &wreq::Client, url: &str) -> Result<Vec<u8>> {
    client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to request {url}"))?
        .error_for_status()
        .with_context(|| format!("request failed for {url}"))?
        .bytes()
        .await
        .with_context(|| format!("failed to read body of {url}"))
        .map(|b| b.to_vec())
}

/// Download a full scribd document to `filename` (one PDF page per scribd page).
///
/// Progress is reported in units of completed stages over `2 * pages` total
/// (JSONP fetch + image download), when `progress_tx` is given.
pub async fn download_scribd_pdf(
    client: &wreq::Client,
    mp: Option<&indicatif::MultiProgress>,
    progress_tx: Option<tokio::sync::mpsc::Sender<ProgressUpdate>>,
    html: &str,
    filename: &str,
) -> Result<()> {
    let jsonp_urls = extract_page_jsonp_urls(html)?;
    if let Some(mp) = mp {
        let _ = mp.println(format!(
            "Found {} scribd pages, downloading…",
            jsonp_urls.len()
        ));
    }
    let pb = mp.map(|mp| {
        let pb = mp.add(indicatif::ProgressBar::new(jsonp_urls.len() as u64));
        pb.set_style(
            indicatif::ProgressStyle::with_template(
                "{msg} [{wide_bar:.cyan/blue}] {pos}/{len} ({eta})",
            )
            .expect("valid progress template")
            .progress_chars("#>-"),
        );
        pb.set_message(filename.to_string());
        pb
    });

    // Stage 1: fetch JSONP payloads concurrently, keep page order.
    let sem = Arc::new(tokio::sync::Semaphore::new(PAGE_CONCURRENCY));
    let mut set = tokio::task::JoinSet::new();
    for (idx, url) in jsonp_urls.iter().enumerate() {
        let client = client.clone();
        let url = url.clone();
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await;
            let body = fetch_bytes(&client, &url).await?;
            let text = String::from_utf8(body)
                .with_context(|| format!("non-UTF8 JSONP payload: {url}"))?;
            anyhow::Ok((idx, text))
        });
    }
    let mut jsonps: Vec<Option<String>> = vec![];
    jsonps.resize_with(jsonp_urls.len(), || None);
    let mut done: u64 = 0;
    let total = (jsonp_urls.len() as u64) * 2;
    while let Some(joined) = set.join_next().await {
        let (idx, text) = joined.map_err(|e| anyhow::anyhow!("page task failed: {e}"))??;
        jsonps[idx] = Some(text);
        done += 1;
        if let Some(tx) = &progress_tx {
            let _ = tx
                .try_send(ProgressUpdate {
                    downloaded: done,
                    total: Some(total),
                })
                .ok();
        }
    }
    let mut image_urls = Vec::with_capacity(jsonp_urls.len());
    for (i, payload) in jsonps.into_iter().enumerate() {
        let payload =
            payload.with_context(|| format!("missing JSONP payload for page {}", i + 1))?;
        image_urls.push(extract_page_image_url(&payload)?);
    }

    // Stage 2: download page images concurrently, keep page order.
    let mut set = tokio::task::JoinSet::new();
    for (idx, url) in image_urls.into_iter().enumerate() {
        let client = client.clone();
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await;
            let bytes = fetch_bytes(&client, &url).await?;
            anyhow::Ok((idx, bytes))
        });
    }
    let mut images: Vec<Option<Vec<u8>>> = vec![];
    images.resize_with(jsonp_urls.len(), || None);
    while let Some(joined) = set.join_next().await {
        let (idx, bytes) = joined.map_err(|e| anyhow::anyhow!("image task failed: {e}"))??;
        images[idx] = Some(bytes);
        if let Some(pb) = &pb {
            pb.inc(1);
        }
        done += 1;
        if let Some(tx) = &progress_tx {
            let _ = tx
                .try_send(ProgressUpdate {
                    downloaded: done,
                    total: Some(total),
                })
                .ok();
        }
    }

    let mut pages = Vec::with_capacity(jsonp_urls.len());
    for (i, bytes) in images.into_iter().enumerate() {
        let bytes = bytes.with_context(|| format!("missing image for page {}", i + 1))?;
        let (w, h, comp) = jpeg_dimensions(&bytes)
            .with_context(|| format!("page {} is not a valid JPEG", i + 1))?;
        pages.push(PdfPage {
            jpeg: bytes,
            width: w,
            height: h,
            components: comp,
        });
    }

    let pdf = build_pdf(&pages)?;
    let part = format!("{filename}.part");
    tokio::fs::write(&part, &pdf)
        .await
        .with_context(|| format!("failed to write {part}"))?;
    tokio::fs::rename(&part, filename)
        .await
        .with_context(|| format!("failed to rename {part} to {filename}"))?;
    if let Some(pb) = &pb {
        pb.finish_with_message(format!("Downloaded {filename}"));
    } else if let Some(mp) = mp {
        let _ = mp.println(format!("Downloaded {filename}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTML: &str = r#"<html><body><script>
var p1 = {contentUrl: "https://html.scribdassets.com/abc/pages/2-def.jsonp"};
var p2 = {contentUrl: "https://html.scribdassets.com/abc/pages/1-xyz.jsonp"};
var p3 = {contentUrl: "https://html.scribdassets.com/abc/pages/10-aaa.jsonp"};
</script></body></html>"#;

    #[test]
    fn extracts_jsonp_urls_in_page_order() {
        let urls = extract_page_jsonp_urls(HTML).unwrap();
        assert_eq!(
            urls,
            vec![
                "https://html.scribdassets.com/abc/pages/1-xyz.jsonp",
                "https://html.scribdassets.com/abc/pages/2-def.jsonp",
                "https://html.scribdassets.com/abc/pages/10-aaa.jsonp",
            ]
        );
    }

    #[test]
    fn errors_when_no_jsonp_urls() {
        assert!(extract_page_jsonp_urls("<html><body>hi</body></html>").is_err());
    }

    #[test]
    fn challenge_page_suggests_cookies() {
        let err = extract_page_jsonp_urls(
            "<html><head><title>Client Challenge</title></head><body id=\"_fs-ch\"></body></html>",
        )
        .unwrap_err();
        assert!(format!("{err:?}").contains("--cookie"), "got: {err:?}");
    }

    const JSONP_ESCAPED: &str = r#"window.page1_callback(["<div><img orig=\"http://html.scribd.com/abc/images/1-xyz.jpg\"/></div>"]);"#;
    const JSONP_PLAIN: &str = r#"<img orig="http://html.scribd.com/abc/images/5-qrs.jpg"/>"#;

    #[test]
    fn extracts_escaped_image_url_upgraded_to_https() {
        assert_eq!(
            extract_page_image_url(JSONP_ESCAPED).unwrap(),
            "https://html.scribd.com/abc/images/1-xyz.jpg"
        );
    }

    #[test]
    fn extracts_plain_image_url() {
        assert_eq!(
            extract_page_image_url(JSONP_PLAIN).unwrap(),
            "https://html.scribd.com/abc/images/5-qrs.jpg"
        );
    }

    #[test]
    fn errors_when_no_image_url() {
        assert!(extract_page_image_url("window.page1_callback([]);").is_err());
    }

    /// Minimal 2x1 RGB JPEG: SOI + SOF0 + EOI.
    fn tiny_jpeg() -> Vec<u8> {
        vec![
            0xFF, 0xD8, // SOI
            0xFF, 0xE0, 0x00, 0x10, // APP0 len 16
            0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00,
            0x00, // APP0 body
            0xFF, 0xC0, 0x00, 0x11, // SOF0 len 17
            0x08, 0x00, 0x01, 0x00, 0x02, // prec 8, h 1, w 2
            0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01, // 3 comps
            0xFF, 0xD9, // EOI
        ]
    }

    #[test]
    fn reads_jpeg_dimensions() {
        assert_eq!(jpeg_dimensions(&tiny_jpeg()).unwrap(), (2, 1, 3));
    }

    #[test]
    fn rejects_non_jpeg() {
        assert!(jpeg_dimensions(b"%PDF-1.4 nope").is_err());
    }

    fn test_pages() -> Vec<PdfPage> {
        vec![
            PdfPage {
                jpeg: tiny_jpeg(),
                width: 2,
                height: 1,
                components: 3,
            },
            PdfPage {
                jpeg: tiny_jpeg(),
                width: 2,
                height: 1,
                components: 1,
            },
        ]
    }

    #[test]
    fn builds_valid_pdf_shell() {
        let pdf = build_pdf(&test_pages()).unwrap();
        assert!(pdf.starts_with(b"%PDF-1.4\n"));
        assert!(pdf.ends_with(b"%%EOF"));
        // Two /Type /Page entries (the /Pages container uses "/Type /Pages").
        let pages = pdf.windows(12).filter(|w| *w == b"/Type /Page ").count();
        assert_eq!(pages, 2);
        assert!(pdf.windows(12).any(|w| w == b"/ColorSpace "));
        // xref offsets must point at "N 0 obj".
        let text = String::from_utf8_lossy(&pdf);
        let after_xref = text.split("xref\n").nth(1).unwrap();
        let mut lines = after_xref.lines();
        let _range: Vec<&str> = lines.next().unwrap().split_whitespace().collect();
        lines.next(); // free entry 0
        for (i, line) in lines.enumerate() {
            if line.starts_with("trailer") {
                break;
            }
            let off: usize = line[..10].parse().unwrap();
            let n = i + 1;
            assert!(
                pdf[off..].starts_with(format!("{n} 0 obj").as_bytes()),
                "xref entry {n} points at wrong offset"
            );
        }
    }

    #[test]
    fn refuses_empty_pages() {
        assert!(build_pdf(&[]).is_err());
    }
}
