# academia-dl

Download PDFs from academia.edu and scribd.com without logging in or creating an account.

Scribd documents are reconstructed page-by-page (each page image is
downloaded anonymously and embedded into a single PDF, one page per image).

## Prerequisites

Install Rust via [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

The HTTP client compiles BoringSSL from source, so you also need a C/C++
toolchain, cmake, and libclang. First build is slow; later builds reuse `target/`.

```bash
# Debian / Ubuntu
sudo apt install -y build-essential cmake libclang-dev

# Fedora
sudo dnf install -y gcc gcc-c++ make cmake clang-devel

# Arch
sudo pacman -S base-devel cmake clang

# macOS (libclang ships with Xcode tools)
xcode-select --install
brew install cmake
```

## Usage

Build with [Rust](https://www.rust-lang.org/) (`cargo build --release`)
then run:

```bash
./target/release/academia-dl "https://www.academia.edu/rest/of/url"
./target/release/academia-dl "https://www.scribd.com/document/519991367/ISO-5807-1985"
```

Multiple URLs (either host) download concurrently, each with its own progress bar:

```bash
academia-dl "https://www.academia.edu/url/one" "https://www.scribd.com/document/..."
```

Or install once: `cargo install --path .`

## Docker Usage

```bash
docker build -t academia-dl .
docker run -ti -v "$(pwd)":/data academia-dl "https://www.academia.edu/rest/of/url"
```

## Cookies (optional)

Not needed in normal use: the client emulates Chrome's TLS fingerprint,
which is what passes Cloudflare (plain HTTP clients get HTTP 403).

If you ever hit rate limits or a login-gated paper, pass a logged-in
browser session explicitly:

```bash
academia-dl --cookie "cf_clearance=VALUE; other=VALUE" "https://www.academia.edu/..."
# or export once:
export ACADEMIA_COOKIES="cf_clearance=VALUE; other=VALUE"
```

To export: DevTools → Network tab → refresh → click the paper request →
Headers → copy the `Cookie:` value (cookies are HttpOnly, invisible to
`document.cookie` in the console).
