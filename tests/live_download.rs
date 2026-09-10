//! Live end-to-end test. Ignored by default (needs network + academia.edu reachable
//! without bot-block). Run: `cargo test -- --ignored`
#[test]
#[ignore]
fn downloads_pdf_with_magic_bytes() {
    // Runs the built binary against a known public URL and checks %PDF header.
    let bin = option_env!("CARGO_BIN_EXE_academia-dl")
        .map(String::from)
        .unwrap_or_else(|| String::from("target/debug/academia-dl"));
    let mut cmd = std::process::Command::new(bin);
    if let Ok(cookies) = std::env::var("ACADEMIA_COOKIES") {
        let cookies = cookies.trim().to_string();
        if !cookies.is_empty() {
            cmd.arg("--cookie").arg(cookies);
        }
    }
    let out = cmd
        .arg("https://www.academia.edu/41779665/Database_Systems_A_Practical_A_Thomas_Connolly")
        .output()
        .expect("failed to run binary");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
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
