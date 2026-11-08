#!/bin/sh
# Install quire on Linux or macOS:
#
#   curl -fsSL https://github.com/irfan-khan-96/quire/releases/latest/download/install.sh | sh
#
# Environment variables:
#   QUIRE_VERSION       release tag to install, e.g. v0.1.0 (default: latest)
#   QUIRE_INSTALL_DIR   where to put the binary (default: ~/.quire/bin)
#   QUIRE_TARGET        override the detected platform, e.g. x86_64-unknown-linux-musl
#   QUIRE_DOWNLOAD_URL  base URL or local directory holding the release files
set -eu

repo="irfan-khan-96/quire"
version="${QUIRE_VERSION:-latest}"
install_dir="${QUIRE_INSTALL_DIR:-$HOME/.quire/bin}"

fail() {
    echo "error: $*" >&2
    exit 1
}

detect_target() {
    case "$(uname -s)" in
        Linux) os="unknown-linux-musl" ;;
        Darwin) os="apple-darwin" ;;
        *) fail "unsupported system $(uname -s); on Windows, use install.ps1" ;;
    esac
    case "$(uname -m)" in
        x86_64 | amd64) arch="x86_64" ;;
        arm64 | aarch64) arch="aarch64" ;;
        *) fail "unsupported architecture $(uname -m)" ;;
    esac
    echo "$arch-$os"
}

fetch() {
    case "$base" in
        http://* | https://*)
            if command -v curl > /dev/null 2>&1; then
                curl -fsSL "$base/$1" -o "$2" || fail "could not download $base/$1"
            elif command -v wget > /dev/null 2>&1; then
                wget -q "$base/$1" -O "$2" || fail "could not download $base/$1"
            else
                fail "curl or wget is required"
            fi
            ;;
        *) cp "$base/$1" "$2" || fail "could not copy $base/$1" ;;
    esac
}

sha256() {
    if command -v sha256sum > /dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

target="${QUIRE_TARGET:-$(detect_target)}"
if [ -n "${QUIRE_DOWNLOAD_URL:-}" ]; then
    base="$QUIRE_DOWNLOAD_URL"
elif [ "$version" = "latest" ]; then
    base="https://github.com/$repo/releases/latest/download"
else
    base="https://github.com/$repo/releases/download/$version"
fi
archive="quire-$target.tar.gz"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

echo "Downloading quire ($version, $target)"
fetch "$archive" "$tmp/$archive"
fetch "$archive.sha256" "$tmp/$archive.sha256"

expected="$(cut -d ' ' -f 1 < "$tmp/$archive.sha256")"
actual="$(sha256 "$tmp/$archive")"
[ "$expected" = "$actual" ] || fail "checksum mismatch for $archive (expected $expected, got $actual)"

tar xzf "$tmp/$archive" -C "$tmp"
mkdir -p "$install_dir"
cp "$tmp/quire-$target/quire" "$install_dir/quire"
chmod +x "$install_dir/quire"
echo "Installed quire to $install_dir/quire"

case ":$PATH:" in
    *":$install_dir:"*) ;;
    *)
        echo
        echo "Add it to your PATH, e.g. in ~/.bashrc or ~/.zshrc:"
        echo "  export PATH=\"$install_dir:\$PATH\""
        ;;
esac
echo "Then run: quire doctor"
