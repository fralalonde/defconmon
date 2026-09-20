#!/usr/bin/env sh
set -eu

# Installer for defconmon (x86_64 Linux only). POSIX-sh compatible because the
# documented invocation is `curl ... | sh`. Do not use Bash-only features here.
#
# A release ships three archives:
#   defconmon-<VER>-x86_64-linux-cpu     # lean CPU-rendered display
#   defconmon-<VER>-x86_64-linux-gpu     # GPU-accelerated display (EGL/Wayland via dlopen)
#   defconmon-config-<VER>-x86_64-linux  # config server
#
# This installer fetches one display archive (cpu or gpu) plus the config
# server, and drops both binaries into PREFIX/bin.

REPOSITORY="${DEFCONMON_REPOSITORY:-fralalonde/defconmon}"
PREFIX="${DEFCONMON_PREFIX:-/usr/local}"
VARIANT="${DEFCONMON_VARIANT:-gpu}"
VERSION="${DEFCONMON_VERSION:-}"
DOWNLOAD_BASE="${DEFCONMON_DOWNLOAD_BASE_URL:-}"

usage() { cat <<'EOF'
Usage: install.sh [--variant cpu|gpu] [--prefix DIR] [--version VER] [--repository REPO]

Installs the defconmon display binary (CPU or GPU variant) and the config
server below PREFIX/bin (default /usr/local/bin). x86_64 Linux only.
EOF
}
while [ "$#" -gt 0 ]; do
  case "$1" in
    --variant) VARIANT="${2:?--variant needs a value}"; shift 2 ;;
    --prefix) PREFIX="${2:?--prefix needs a value}"; shift 2 ;;
    --version) VERSION="${2:?--version needs a value}"; shift 2 ;;
    --repository) REPOSITORY="${2:?--repository needs a value}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'Unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
done

case "$VARIANT" in
  cpu|gpu) ;;
  *) echo "Variant must be 'cpu' or 'gpu', got: $VARIANT" >&2; exit 2 ;;
esac
[ "$(uname -s)" = Linux ] || { echo "defconmon only supports Linux." >&2; exit 1; }
case "$(uname -m)" in
  x86_64|amd64) ;;
  *) echo "defconmon only supports x86_64 on Linux, got: $(uname -m)" >&2; exit 1 ;;
esac

if [ -z "$VERSION" ]; then
  VERSION="$(curl --fail --silent --show-error --location "https://api.github.com/repos/$REPOSITORY/releases/latest" | tr -d '\r' | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\([^"]*\)".*/\1/p' | head -n1)"
fi
[ -n "$VERSION" ] || { echo 'Unable to determine defconmon version; pass --version VER.' >&2; exit 1; }

display_asset="defconmon-${VERSION}-x86_64-linux-${VARIANT}.tar.gz"
config_asset="defconmon-config-${VERSION}-x86_64-linux.tar.gz"
base="${DOWNLOAD_BASE:-https://github.com/$REPOSITORY/releases/download/v$VERSION}"

tmp="$(mktemp -d)"
cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT

for asset in "$display_asset" "$config_asset"; do
  curl --fail --location --retry 3 --output "$tmp/$asset" "$base/$asset"
done
if curl --fail --silent --show-error --location --output "$tmp/checksums.txt" "$base/checksums.txt"; then
  (cd "$tmp" && sha256sum -c --ignore-missing checksums.txt) || {
    echo "checksum verification failed" >&2; exit 1; }
fi

mkdir -p "$tmp/unpack"
tar -xzf "$tmp/$display_asset" -C "$tmp/unpack"
tar -xzf "$tmp/$config_asset" -C "$tmp/unpack"
# The display archive's root is the variant name (cpu|gpu); the config
# archive's root is "config". Each holds exactly one binary.
mkdir -p "$PREFIX/bin"
install -m 0755 "$tmp/unpack/$VARIANT/defconmon" "$PREFIX/bin/defconmon"
install -m 0755 "$tmp/unpack/config/defconmon-config" "$PREFIX/bin/defconmon-config"

printf '\nInstalled defconmon %s (%s) and defconmon-config to %s/bin\n' "$VERSION" "$VARIANT" "$PREFIX"
printf 'Set DEFCONMON_VARIANT=cpu for the lean CPU build.\n'