use anyhow::{bail, Result};
use url::Url;

pub fn validate_academia_url(input: &str) -> Result<Url> {
    let u = Url::parse(input).map_err(|_| anyhow::anyhow!("Error parsing URL: {input}"))?;
    if u.scheme() != "http" && u.scheme() != "https" {
        bail!("Error parsing URL: {input}");
    }
    let host = u.host_str().unwrap_or("");
    if host != "academia.edu" && !host.ends_with(".academia.edu") {
        bail!("URL host must be 'academia.edu', error with URL: {input}");
    }
    if u.path().is_empty() || u.path() == "/" {
        bail!("Error parsing URL: {input}");
    }
    Ok(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_academia_edu_and_subdomain() {
        assert!(validate_academia_url("https://www.academia.edu/12345/Some_Title").is_ok());
        assert!(validate_academia_url("https://foo.academia.edu/12345/x").is_ok());
        assert!(validate_academia_url("http://academia.edu/12345/x").is_ok());
    }

    #[test]
    fn rejects_bad_scheme_host_and_empty_path() {
        assert!(validate_academia_url("ftp://www.academia.edu/123/x").is_err());
        assert!(validate_academia_url("https://www.evilacademia.edu/123/x").is_err());
        assert!(validate_academia_url("https://www.example.com/123/x").is_err());
        assert!(validate_academia_url("https://www.academia.edu").is_err());
        assert!(validate_academia_url("https://www.academia.edu/").is_err());
        assert!(validate_academia_url("not a url").is_err());
    }
}
