use url::Url;

pub fn filename_from_url(u: &Url) -> String {
    let raw = u
        .path_segments()
        .and_then(|mut s| s.rfind(|x| !x.is_empty()))
        .unwrap_or("download");
    let sanitized: String = raw.replace(['/', '\\'], "_");
    let stem: String = if sanitized == "." || sanitized == ".." || sanitized.is_empty() {
        "download".to_string()
    } else {
        sanitized.chars().take(251).collect()
    };
    format!("{stem}.pdf")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_last_segment_plus_pdf() {
        let u = url::Url::parse("https://www.academia.edu/41779665/Database_Systems_A_Practical")
            .unwrap();
        assert_eq!(filename_from_url(&u), "Database_Systems_A_Practical.pdf");
    }

    #[test]
    fn truncates_to_251_chars_plus_pdf() {
        let long = "a".repeat(400);
        let u = url::Url::parse(&format!("https://www.academia.edu/1/{long}")).unwrap();
        let name = filename_from_url(&u);
        assert_eq!(name.len(), 251 + 4);
        assert!(name.ends_with(".pdf"));
    }

    #[test]
    fn trailing_slash_uses_last_nonempty_segment() {
        let u = url::Url::parse("https://www.academia.edu/41779665/Database_Systems").unwrap();
        let u2 = url::Url::parse("https://www.academia.edu/41779665/Database_Systems/").unwrap();
        assert_eq!(filename_from_url(&u2), filename_from_url(&u));
        assert_eq!(filename_from_url(&u2), "Database_Systems.pdf");
    }

    #[test]
    fn sanitizes_path_traversal() {
        let u = url::Url::parse("https://www.academia.edu/1/%2e%2e%2fetc%2fpasswd").unwrap();
        let name = filename_from_url(&u);
        assert!(!name.contains('/'));
        assert!(!name.contains(".."));
    }
}
