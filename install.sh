#!/bin/sh
set -eu

repo="${LOCUS_REPO:-alkinun/locus}"
version="${LOCUS_VERSION:-latest}"
bin_dir="${LOCUS_INSTALL_DIR:-$HOME/.local/bin}"
binary="locus"

need_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "error: required command not found: $1" >&2
        exit 1
    fi
}

download() {
    url="$1"
    out="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$out"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$out" "$url"
    else
        echo "error: either curl or wget is required" >&2
        exit 1
    fi
}

checksum() {
    file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print $1}'
    else
        return 1
    fi
}

case "$(uname -s)" in
    Linux) os="unknown-linux-gnu" ;;
    Darwin) os="apple-darwin" ;;
    *)
        echo "error: unsupported operating system: $(uname -s)" >&2
        exit 1
        ;;
esac

case "$(uname -m)" in
    x86_64 | amd64) arch="x86_64" ;;
    arm64 | aarch64) arch="aarch64" ;;
    *)
        echo "error: unsupported CPU architecture: $(uname -m)" >&2
        exit 1
        ;;
esac

target="${arch}-${os}"
asset="${binary}-${target}.tar.gz"

if [ "$version" = "latest" ]; then
    base_url="https://github.com/${repo}/releases/latest/download"
else
    base_url="https://github.com/${repo}/releases/download/${version}"
fi

tmp_dir="$(mktemp -d 2>/dev/null || mktemp -d -t locus)"
trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM

archive="${tmp_dir}/${asset}"
checksum_file="${tmp_dir}/${asset}.sha256"

echo "Downloading ${asset}..."
download "${base_url}/${asset}" "$archive"

if download "${base_url}/${asset}.sha256" "$checksum_file"; then
    expected="$(awk '{print $1}' "$checksum_file")"
    if actual="$(checksum "$archive")"; then
        if [ "$actual" != "$expected" ]; then
            echo "error: checksum mismatch for ${asset}" >&2
            echo "expected: $expected" >&2
            echo "actual:   $actual" >&2
            exit 1
        fi
    else
        echo "warning: sha256sum/shasum not found; skipping checksum verification" >&2
    fi
else
    echo "warning: checksum file unavailable; skipping checksum verification" >&2
fi

need_cmd tar
mkdir -p "$bin_dir"
tar -xzf "$archive" -C "$tmp_dir"

if command -v install >/dev/null 2>&1; then
    install -m 755 "${tmp_dir}/${binary}" "${bin_dir}/${binary}"
else
    cp "${tmp_dir}/${binary}" "${bin_dir}/${binary}"
    chmod 755 "${bin_dir}/${binary}"
fi

echo "Installed ${binary} to ${bin_dir}/${binary}"
case ":$PATH:" in
    *:"$bin_dir":*) ;;
    *) echo "Add ${bin_dir} to PATH to run ${binary} from any directory." ;;
esac
