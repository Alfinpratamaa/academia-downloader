# AGENTS.md

Rust CLI (`src/`, one module per pipeline stage) that downloads PDFs from
academia.edu. Single binary, async tokio, concurrent multi-URL via `JoinSet`.
Full context in `CLAUDE.md`; design/plan history in `docs/superpowers/`.

## Commands

- Build / run: `cargo build` / `cargo run -- "<url>"` (release: `cargo build --release`)
- Unit tests (pure, no network): `cargo test`
- Single test: `cargo test <name>` (e.g. `cargo test prefers_id_followed_by_identifier`)
- Live e2e (network, needs cookies): `cargo test -- --ignored`
- Lint / fmt: `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`
- Docker: `docker build -t academia-dl .`, `docker run -ti -v "$(pwd)":/data academia-dl "<url>"`

## Setup gotchas

- `wreq` compiles BoringSSL via `btls-sys`: needs `libclang-dev` + C/C++ toolchain + cmake, or the build fails in bindgen. First build is slow (`target/` caches it).
- Live runs need `ACADEMIA_COOKIES` (or `--cookie`) from a logged-in browser session. Cookies are HttpOnly — export via DevTools → Network → copy `Cookie:` header, not `document.cookie`.

## Order-sensitive / non-obvious

- `fetch.rs`: `wreq` `.emulation(...)` MUST precede `.default_headers(...)` (emulation overwrites them). Plain reqwest/curl 403 even with valid cookies — only Chrome-emulated TLS fingerprint passes Cloudflare.
- `fetch.rs`: connect-only timeout. Never add a total request timeout; it kills slow large downloads mid-stream.
- `main.rs`: one shared `indicatif::MultiProgress`; status lines via `mp.println`, never `eprintln` (glitches bars). Never `exit()` inside a `JoinSet` task — return `Err`, `main` collects and exits 1.
- `download.rs`: stream to `<name>.part`, rename on success, delete `.part` on error — otherwise a partial file is mistaken for "already exists, skipping".
- `parse.rs`: `[{"id":` matches author entities too; the download entity is the one whose bounded context (capped at next `[{"id":`, else 600 chars) contains `"identifier"`. Prefer embedded `"downloadUrl"` when present (direct `attachments/<id>/download_file` link, logged-in pages). Debug with `ACADEMIA_DEBUG=1`.
- `filename.rs`: sanitize `/` `\` `..` (the `url` crate percent-decodes; Ruby original didn't), truncate 251 chars + `.pdf`.
- wreq merges `default_headers` at `execute()` time — unit tests assert on the `HeaderMap` from `default_headers()`, not a built request.
- `wreq`/`wreq-util` are RC pins in `Cargo.lock`; `wreq::Client::get` takes `IntoUri` (`&str`), not `url::Url` — pass `url.as_str()`.
