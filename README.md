# academia-dl

Download PDFs from academia dot edu (must login and take the cookies)

## Usage

Build with [Rust](https://www.rust-lang.org/) 

```bash
cargo build --release
```

Then run:
```bash
./target/release/academia-dl "https://www.academia.edu/rest/of/url"
```

Multiple URLs are downloaded concurrently:
```bash
academia-dl "https://www.academia.edu/url/one" "https://www.academia.edu/url/two"
```

## Docker Usage

    docker build -t academia-dl .
    docker run -ti -v "$(pwd)":/data academia-dl "https://www.academia.edu/rest/of/url"


## Cookies (required — Cloudflare + login)

Anonymous downloads are blocked (HTTP 403, Cloudflare challenge, login-gated
buttons). Steps:
1. Log in at academia.edu in your browser and open any paper page.
2. DevTools (F12) → Application → Cookies → https://www.academia.edu.
3. Copy values of `cf_clearance` (and any `academia.edu_session`-like cookie) as:
   `cf_clearance=VALUE; other=VALUE`
4. Run: `academia-dl --cookie "cf_clearance=VALUE; other=VALUE" "https://www.academia.edu/..."`
   or export once: `export ACADEMIA_COOKIES="cf_clearance=VALUE; other=VALUE"`
