#!/usr/bin/env bash
# Installs the external QA tools at the versions pinned in tools.lock.
#
# Usage: scripts/install-tools.sh [--check]
#   --check  only report missing or mismatched tools; exit 1 if any.
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lock="${repo_dir}/tools.lock"
check_only=0
[[ "${1:-}" == "--check" ]] && check_only=1

# Prints "section name version" for every pinned tool.
pins() {
    awk '
        /^\[/ { gsub(/[\[\]]/, ""); section = $0; next }
        /^[a-z]/ { gsub(/"/, "", $3); print section, $1, $3 }
    ' "${lock}"
}

# Version reported by an installed cargo tool, or empty.
installed_version() {
    local name="$1"
    cargo install --list | awk -v n="${name}" '$1 == n { gsub(/[v:]/, "", $2); print $2 }'
}

ensure_binstall() {
    local version="$1"
    if [[ "$(installed_version cargo-binstall)" == "${version}" ]]; then
        return
    fi
    local work
    work="$(mktemp -d)"
    trap 'rm -rf "${work}"' RETURN
    local url="https://github.com/cargo-bins/cargo-binstall/releases/download/v${version}/cargo-binstall-x86_64-unknown-linux-musl.tgz"
    curl --proto '=https' --tlsv1.2 -sSfL -o "${work}/binstall.tgz" "${url}"
    tar -xzf "${work}/binstall.tgz" -C "${work}"
    "${work}/cargo-binstall" --no-confirm "cargo-binstall@${version}"
}

missing=0
while read -r section name version; do
    current="$(installed_version "${name}")"
    if [[ "${current}" == "${version}" ]]; then
        echo "ok        ${name} ${version}"
        continue
    fi
    if (( check_only )); then
        echo "MISSING   ${name} ${version} (installed: ${current:-none})"
        missing=1
        continue
    fi
    case "${section}" in
        bootstrap) ensure_binstall "${version}" ;;
        prebuilt) cargo binstall --no-confirm --locked "${name}@${version}" ;;
        source) cargo install --locked --force "${name}@${version}" ;;
        *) echo "unknown section ${section}" >&2; exit 2 ;;
    esac
done < <(pins | sort -s -k1,1)

exit "${missing}"
