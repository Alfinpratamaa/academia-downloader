use anyhow::{Context, Result};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

pub const PREFIX: &str = "https://www.academia.edu/download";

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProgressUpdate {
    pub downloaded: u64,
    pub total: Option<u64>,
}

pub fn download_url(download_id: &str, filename: &str) -> String {
    format!("{PREFIX}/{download_id}/{filename}")
}

pub async fn download_to_file(
    client: &wreq::Client,
    mp: Option<&indicatif::MultiProgress>,
    progress_tx: Option<tokio::sync::mpsc::Sender<ProgressUpdate>>,
    url: &str,
    filename: &str,
) -> Result<()> {
    if let Some(mp) = mp {
        let _ = mp.println(format!("Resolved download URL: {url}"));
    }
    let part = format!("{filename}.part");
    let result: Result<()> = async {
        let resp = client
            .get(url)
            .send()
            .await
            .with_context(|| format!("failed to request download URL {url}"))?
            .error_for_status()
            .with_context(|| format!("download request failed for URL {url}"))?;
        let total = resp.content_length();
        let pb = mp.map(|mp| {
            let pb = mp.add(indicatif::ProgressBar::new(total.unwrap_or(0)));
            pb.set_style(
                indicatif::ProgressStyle::with_template(
                    "{msg} [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})",
                )
                .expect("valid progress template")
                .progress_chars("#>-"),
            );
            pb.set_message(filename.to_string());
            pb
        });
        let mut file = tokio::fs::File::create(&part)
            .await
            .with_context(|| format!("failed to create file {part}"))?;
        let mut downloaded: u64 = 0;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream
            .next()
            .await
            .transpose()
            .context("failed while streaming download body")?
        {
            downloaded += chunk.len() as u64;
            if let Some(pb) = &pb {
                pb.inc(chunk.len() as u64);
            }
            if let Some(tx) = &progress_tx {
                let _ = tx.try_send(ProgressUpdate { downloaded, total });
            }
            file.write_all(&chunk)
                .await
                .with_context(|| format!("failed while writing {part}"))?;
        }
        file.flush().await?;
        drop(file);
        tokio::fs::rename(&part, filename)
            .await
            .with_context(|| format!("failed to rename {part} to {filename}"))?;
        if let Some(pb) = &pb {
            pb.finish_with_message(format!("Downloaded {filename}"));
        }
        Ok(())
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&part).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_download_url() {
        assert_eq!(
            download_url("41779665", "Database.pdf"),
            "https://www.academia.edu/download/41779665/Database.pdf"
        );
    }
}
