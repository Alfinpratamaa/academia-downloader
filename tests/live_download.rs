//! Live end-to-end tests. Ignored by default (need network + hosts reachable
//! without bot-block). Run: `cargo test -- --ignored`
fn run_dl(args: &[&str]) -> std::process::Output {
    let bin = option_env!("CARGO_BIN_EXE_academia-dl")
        .map(String::from)
        .unwrap_or_else(|| String::from("target/debug/academia-dl"));
    std::process::Command::new(bin)
        .args(args)
        .output()
        .expect("failed to run binary")
}

fn assert_fresh_pdf() {
    let pdfs: Vec<_> = std::fs::read_dir(".")
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "pdf").unwrap_or(false))
        .collect();
    assert!(!pdfs.is_empty(), "no pdf downloaded");
    for p in &pdfs {
        let mut f = std::fs::File::open(p).unwrap();
        let mut magic = [0u8; 4];
        use std::io::Read;
        f.read_exact(&mut magic).unwrap();
        assert_eq!(&magic, b"%PDF");
        std::fs::remove_file(p).unwrap();
    }
}

#[test]
#[ignore]
fn downloads_pdf_with_magic_bytes() {
    // Runs the built binary against a known public URL and checks %PDF header.
    let mut args = vec![];
    let cookies;
    if let Ok(c) = std::env::var("ACADEMIA_COOKIES") {
        let c = c.trim().to_string();
        if !c.is_empty() {
            cookies = c;
            args.push("--cookie");
            args.push(&cookies);
        }
    }
    args.push("https://www.academia.edu/41779665/Database_Systems_A_Practical_A_Thomas_Connolly");
    let out = run_dl(&args);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_fresh_pdf();
}

#[test]
#[ignore]
fn downloads_scribd_pdf_without_auth() {
    // Public scribd document; must work with no cookies at all.
    let out = run_dl(&["https://www.scribd.com/document/519991367/ISO-5807-1985"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_fresh_pdf();
}
