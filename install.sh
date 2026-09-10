#!/usr/bin/env bash
# academia-dl one-line installer.
# Usage: curl -fsSL https://raw.githubusercontent.com/Alfinpratamaa/academia-downloader/main/install.sh | sudo bash
set -euo pipefail

REPO_URL="${REPO_URL:-https://github.com/Alfinpratamaa/academia-downloader.git}"
INSTALL_DIR="${INSTALL_DIR:-/opt/academia-dl}"
IMAGE="${IMAGE:-academia-dl}"

if [ "$(id -u)" -ne 0 ]; then
    echo "Re-running with sudo..." >&2
    exec sudo -E bash "$0" "$@"
fi

have() { command -v "$1" >/dev/null 2>&1; }

install_docker() {
    if have docker; then
        echo "[1/4] docker already installed: $(docker --version)"
        return
    fi
    echo "[1/4] installing docker..."
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
    echo "docker installed: $(docker --version)"
}

start_docker() {
    if have systemctl; then
        systemctl enable --now docker >/dev/null 2>&1 || service docker start || true
    elif have service; then
        service docker start || true
    fi
    docker info >/dev/null 2>&1 || {
        echo "ERROR: docker daemon not reachable. Start it manually and re-run." >&2
        exit 1
    }
}

fetch_source() {
    echo "[2/4] fetching source into $INSTALL_DIR..."
    if [ -d "$INSTALL_DIR/.git" ]; then
        git -C "$INSTALL_DIR" pull --ff-only
    else
        rm -rf "$INSTALL_DIR"
        git clone --depth 1 "$REPO_URL" "$INSTALL_DIR"
    fi
}

build_image() {
    echo "[3/4] building docker image '$IMAGE' (first build is slow, BoringSSL compiles)..."
    docker build -t "$IMAGE" "$INSTALL_DIR"
}

install_wrapper() {
    echo "[4/4] installing 'academia-dl' wrapper to /usr/local/bin..."
    cat >/usr/local/bin/academia-dl <<EOF
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
    chmod +x /usr/local/bin/academia-dl
}

install_docker
start_docker
fetch_source
build_image
install_wrapper

echo
echo "Done. Try: academia-dl --help"
academia-dl --help 2>&1 | head -20 || true
