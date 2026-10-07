#!/usr/bin/env bash
# Secret scan with gitleaks (https://github.com/gitleaks/gitleaks), default rule set, redacted
# output. Complements scripts/public-safety-scan.sh: that one catches project-specific terms from an
# external denylist, this one catches credentials, keys and tokens by pattern and entropy.
#
# Usage: scripts/secret-scan.sh [mode]
#   --history   every commit reachable from any ref (default; pre-push, CI, pre-publication audit)
#   --staged    staged changes (pre-commit)
#   --tree      tracked files in the working tree
#
# No suppression route: a tracked .gitleaks.toml or .gitleaksignore is refused and inline
# `gitleaks:allow` comments are ignored. A false positive is fixed by changing the content (for
# example a visibly fake value) or, if that is impossible, by changing this script in review.
# The gitleaks version used in CI is pinned in .github/workflows/public-safety.yml.
set -euo pipefail

if ! command -v gitleaks > /dev/null; then
    echo "secret-scan: gitleaks is not installed (see .github/workflows/public-safety.yml for the pinned version)" >&2
    exit 2
fi
repo_dir="$(git rev-parse --show-toplevel)"
for config in .gitleaks.toml .gitleaksignore; do
    if [[ -e "${repo_dir}/${config}" ]]; then
        echo "secret-scan: ${config} is not allowed; it would let findings be suppressed" >&2
        exit 2
    fi
done

work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT
# An empty ignore directory, so no ignore file from anywhere else applies.
common=(--no-banner --redact --ignore-gitleaks-allow --gitleaks-ignore-path "${work}")

mode="${1:---history}"
case "${mode}" in
    --history)
        gitleaks git "${common[@]}" --log-opts="--all" "${repo_dir}"
        ;;
    --staged)
        gitleaks git "${common[@]}" --pre-commit --staged "${repo_dir}"
        ;;
    --tree)
        mkdir "${work}/tree"
        git -C "${repo_dir}" ls-files -z | while IFS= read -r -d '' file; do
            if [[ -f "${repo_dir}/${file}" ]]; then
                mkdir -p "${work}/tree/$(dirname "${file}")"
                cp "${repo_dir}/${file}" "${work}/tree/${file}"
            fi
        done
        gitleaks dir "${common[@]}" "${work}/tree"
        ;;
    *)
        echo "secret-scan: unknown mode ${mode}" >&2
        exit 2
        ;;
esac
echo "secret-scan: clean (${mode})"
