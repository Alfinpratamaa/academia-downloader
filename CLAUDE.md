# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`academia-dl` — Rust CLI that downloads PDFs from academia.edu without an account.
Rust rewrite of an earlier Ruby tool. Design/plan docs in `docs/superpowers/`.

## Commands

- Build: `cargo build` / `cargo build --release`
- Run: `cargo run -- "https://www.academia.edu/<id>/<slug>"` (multiple URLs → concurrent)
- Unit tests: `cargo test` (all pure, no network)
- Single test: `cargo test extracts_primary_script_id` or `cargo test parse::`
- Live e2e (network, needs working cookies): `cargo test -- --ignored`
- Lint: `cargo clippy --all-targets`
- Docker: `docker build -t academia-dl .` then `docker run -ti -v "$(pwd)":/data academia-dl "<url>"`

## Cookies (optional)

Chrome TLS emulation in `wreq` passes Cloudflare on its own — verified live
with garbage cookies. `--cookie` / `ACADEMIA_COOKIES` still exist as an
escape hatch (rate limits, login-gated papers). If ever needed, export via
DevTools → Network tab → copy the `Cookie:` header (cookies are HttpOnly,
invisible to `document.cookie`). Key ones: `cf_clearance`, `__cf_bm`,
`_cookie_session` (+ `login_token`/`user_id`). Live tests read
`ACADEMIA_COOKIES`.

Plain reqwest/curl 403 even with valid cookies — the fix was fingerprint
(`fetch.rs`), never the cookies.

## Architecture

Pipeline in `src/main.rs::process_one`, one module per stage:

1. `validate.rs` — parse URL, enforce host is `academia.edu` / `*.academia.edu`, non-empty path.
2. `filename.rs` — output name = last non-empty path segment, sanitized (no `/` `\` `..`), truncated 251 chars, `.pdf` appended. Skips download if file already exists.
3. `fetch.rs` — `build_client` (wreq + wreq-util browser emulation) then `fetch_page_html` with retry loop (`MAX_RETRIES` 5, `RETRY_SLEEP_SECS` 5). Connect-only timeout (30s); never a total timeout, or slow large downloads die mid-stream.
4. `parse.rs` — get the download target from page HTML (see below).
5. `download.rs` — `download_to_file` takes `&indicatif::MultiProgress`, draws one progress bar per file (`{msg} [{wide_bar}] {bytes}/{total_bytes} ({eta})`), streams to `<filename>.part`, renames on success, deletes `.part` on error.

`main.rs` dedupes input URLs, creates one `indicatif::MultiProgress` shared (cloned) across tasks, runs each through a `tokio::task::JoinSet`, exits 1 if any fail. Status lines go through `mp.println`, never `eprintln`, so bars don't glitch. `process_one` returns `Err` on invalid URL — never `exit()` inside a task, or sibling downloads die.

### HTTP client (`fetch.rs`) — order matters

`wreq::Client::builder().emulation(...)` MUST come before `.default_headers(...)`.
Emulation overwrites TLS/HTTP2/header config; our UA/Referer/Cookie only survive if set after.
Referer is spoofed to `scholar.google.com`. wreq merges default headers at `execute()` time,
not `build()` time — tests assert on the `HeaderMap` from `default_headers()`, not a built request.

### Download-target resolution (`parse.rs`)

Two paths, tried in order by `process_one`:
1. `extract_download_url` — regex for embedded `"downloadUrl":"..."` (logged-in page variant). Unescapes `\/`. Preferred.
2. `extract_download_id` — find the attachment entity id, then `download.rs::download_url` builds `https://www.academia.edu/download/<id>/<filename>`.

`extract_download_id` fallback chain: `[{"id":\d+` in a `<script>` whose bounded context (window
capped at next `[{"id":` entity, else 600 chars) contains `"identifier"` → first `[{"id":` id in any
script → same regex over raw HTML → `<meta og:url>` id → broad `academia\.edu/(\d+)` over HTML.
The `"identifier"` check disambiguates the real attachment from author/other entities sharing the shape.

Set `ACADEMIA_DEBUG=1` for verbose tracing of pattern counts, candidate ids + context keys, and which branch matched.

## Notes

- Dependencies use RC versions (`wreq` 6.0.0-rc, `wreq-util` 3.0.0-rc) — pinned in `Cargo.lock`.
- Building `wreq` compiles BoringSSL via `btls-sys`, which needs `libclang-dev` (`sudo apt install -y libclang-dev`) plus a C/C++ toolchain and cmake. First build is slow; artifacts cache in `target/`.
- academia.edu HTML structure is undocumented and changes; parser is heuristic. When a URL stops working, run with `ACADEMIA_DEBUG=1` and inspect which fallback fired.
- Recent git history has `debug:` commits dumping page patterns — that is live reverse-engineering of the current page shape, not committed debug code (tracing is env-gated).
