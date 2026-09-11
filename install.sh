#!/usr/bin/env bash
# academia-dl one-line installer.
# Usage: curl -fsSL https://raw.githubusercontent.com/Alfinpratamaa/academia-downloader/main/install.sh | sudo bash
#
# Prefers prebuilt binaries from the latest GitHub release (seconds):
# academia-dl (CLI) + academia-web (web UI).
# Falls back to a local docker build when no release asset fits.
set -euo pipefail

REPO="${REPO:-Alfinpratamaa/academia-downloader}"
REPO_URL="${REPO_URL:-https://github.com/$REPO.git}"
INSTALL_DIR="${INSTALL_DIR:-/opt/academia-dl}"
IMAGE="${IMAGE:-academia-dl}"
BIN_PATH="${BIN_PATH:-/usr/local/bin/academia-dl}"
WEB_BIN_PATH="${WEB_BIN_PATH:-/usr/local/bin/academia-web}"

if [ "$(id -u)" -ne 0 ]; then
    echo "Re-running with sudo..." >&2
    exec sudo -E bash "$0" "$@"
fi

have() { command -v "$1" >/dev/null 2>&1; }

install_base_tools() {
    if have curl && have tar; then return; fi
    echo "Installing curl/tar..."
    if have apt-get; then apt-get update -y && apt-get install -y curl tar ca-certificates
    elif have dnf; then dnf install -y curl tar ca-certificates
    elif have pacman; then pacman -Sy --noconfirm curl tar ca-certificates
    fi
}

arch_asset() {
    case "$(uname -m)" in
        x86_64|amd64) echo "$1-x86_64-unknown-linux-gnu.tar.gz" ;;
        *) echo "" ;;
    esac
}

install_binary() {
    local name dest asset url tmp
    name="$1"
    dest="$2"
    asset="$(arch_asset "$name")"
    [ -n "$asset" ] || return 1
    url="https://github.com/$REPO/releases/latest/download/$asset"
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' RETURN
    echo "Downloading prebuilt binary ($asset)..."
    curl -fsSL "$url" -o "$tmp/pkg.tar.gz" || return 1
    tar -xzf "$tmp/pkg.tar.gz" -C "$tmp"
    install -m 0755 "$tmp/$name" "$dest"
    echo "Installed: $("$dest" --version 2>/dev/null || echo "$name")"
    return 0
}

install_binaries() {
    install_binary academia-dl "$BIN_PATH" || return 1
    install_binary academia-web "$WEB_BIN_PATH" || return 1
}

install_docker() {
    if have docker; then
        echo "docker already installed: $(docker --version)"
        return
    fi
    echo "Installing docker..."
    if have apt-get; then
        apt-get update -y
        apt-get install -y curl git docker.io
    elif have dnf; then
        dnf install -y curl git docker
    elif have pacman; then
        pacman -Sy --noconfirm curl git docker
    else
        curl -fsSL https://get.docker.com | sh
    fi
}

start_docker() {
    if have systemctl; then
        systemctl enable --now docker >/dev/null 2>&1 || service docker start || true
    elif have service; then
        service docker start || true
    fi
    docker info >/dev/null 2>&1 || {
        echo "ERROR: docker daemon not reachable." >&2
        exit 1
    }
}

install_via_docker() {
    echo "No prebuilt binary for this machine; falling back to docker build (slow first time)..."
    if [ -d "$INSTALL_DIR/.git" ]; then
        git -C "$INSTALL_DIR" pull --ff-only || true
    else
        rm -rf "$INSTALL_DIR"
        git clone --depth 1 "$REPO_URL" "$INSTALL_DIR"
    fi
    docker build -t "$IMAGE" "$INSTALL_DIR"
    cat >"$BIN_PATH" <<EOF
#!/usr/bin/env bash
# Thin wrapper: PDFs land in the current directory.
# Optional: export ACADEMIA_COOKIES="..." beforehand for rate-limited papers.
TTY=""; [ -t 1 ] && TTY="-t"
if [ -w /var/run/docker.sock ]; then DOCKER="docker"; else DOCKER="sudo docker"; fi
exec \$DOCKER run --rm -i \$TTY \\
    -v "\$(pwd)":/data \\
    \${ACADEMIA_COOKIES:+ -e ACADEMIA_COOKIES} \\
    $IMAGE "\$@"
EOF
    chmod +x "$BIN_PATH"
}

install_base_tools
if ! install_binaries; then
    install_docker
    start_docker
    install_via_docker
fi

echo
echo "Done. Try: academia-dl --help"
