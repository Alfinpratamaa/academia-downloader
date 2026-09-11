# AGENTS.md

Rust CLI + web UI (`src/`, one module per pipeline stage) that downloads PDFs
from academia.edu and scribd.com. Two binaries (`academia-dl`, `academia-web`),
async tokio, concurrent multi-URL via `JoinSet`.
Full context in `CLAUDE.md`; design/plan history in `docs/superpowers/`.

## Commands

- Build / run: `cargo build` / `cargo run --bin academia-dl -- "<url>"` (release: `cargo build --release`)
- Web UI: `cargo run --bin academia-web` (paste academia.edu or scribd.com URL)
- Unit tests (pure, no network): `cargo test`
- Single test: `cargo test <name>` (e.g. `cargo test prefers_id_followed_by_identifier`)
- Live e2e (network, works anonymously): `cargo test -- --ignored`
- Lint / fmt: `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`
- Docker: `docker build -t academia-dl .`, `docker run -ti -v "$(pwd)":/data academia-dl "<url>"`

## Setup gotchas

- `wreq` compiles BoringSSL via `btls-sys`: needs rustup + C/C++ toolchain + cmake + libclang (per-OS list in README Prerequisites), or the build fails in bindgen. First build is slow (`target/` caches it).
- Live runs work anonymously (wreq Chrome emulation passes Cloudflare — verified live). `ACADEMIA_COOKIES` (or `--cookie`) is an optional escape hatch for rate limits / login-gated papers. To export: DevTools → Network → copy `Cookie:` header (`document.cookie` can't see HttpOnly cookies).

## Order-sensitive / non-obvious

- `fetch.rs`: `wreq` `.emulation(...)` MUST precede `.default_headers(...)` (emulation overwrites them). Plain reqwest/curl 403 even with valid cookies — only Chrome-emulated TLS fingerprint passes Cloudflare.
- `fetch.rs`: connect-only timeout. Never add a total request timeout; it kills slow large downloads mid-stream.
- `main.rs`: one shared `indicatif::MultiProgress`; status lines via `mp.println`, never `eprintln` (glitches bars). Never `exit()` inside a `JoinSet` task — return `Err`, `main` collects and exits 1. Host dispatch lives in `process_one` (CLI) and `run_job`/`create_job` (web): scribd hosts → scribd path, else academia path.
- `download.rs`: stream to `<name>.part`, rename on success, delete `.part` on error — otherwise a partial file is mistaken for "already exists, skipping".
- `parse.rs`: `[{"id":` matches author entities too; the download entity is the one whose bounded context (capped at next `[{"id":`, else 600 chars) contains `"identifier"`. Prefer embedded `"downloadUrl"` when present (direct `attachments/<id>/download_file` link, logged-in pages). Debug with `ACADEMIA_DEBUG=1`.
- `filename.rs`: sanitize `/` `\` `..` (the `url` crate percent-decodes; Ruby original didn't), truncate 251 chars + `.pdf`.
- wreq merges `default_headers` at `execute()` time — unit tests assert on the `HeaderMap` from `default_headers()`, not a built request.
- `wreq`/`wreq-util` are RC pins in `Cargo.lock`; `wreq::Client::get` takes `IntoUri` (`&str`), not `url::Url` — pass `url.as_str()`.
