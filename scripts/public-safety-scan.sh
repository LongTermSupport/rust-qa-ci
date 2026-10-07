#!/usr/bin/env bash
# Public-safety scan: fails when repository content matches any term in a denylist that is kept
# OUTSIDE this repository (so the list itself is never published).
#
# Usage:
#   RQACI_DENYLIST=/path/to/denylist scripts/public-safety-scan.sh [mode]
# Modes:
#   --tree           tracked files in the working tree (default)
#   --staged         staged changes (pre-commit)
#   --message FILE   a commit message file (commit-msg)
#   --history        every commit diff and message reachable from any ref
#   --stdin          text on standard input (issue and PR bodies before posting)
#
# Denylist format: one extended regular expression per line, matched case-insensitively; blank
# lines and lines starting with '#' are ignored. Output names the location and the denylist line
# number, never the matched term, so logs do not republish what the list protects.
set -euo pipefail

denylist="${RQACI_DENYLIST:-}"
if [[ -z "${denylist}" || ! -r "${denylist}" ]]; then
    echo "public-safety-scan: RQACI_DENYLIST must name a readable denylist file outside the repo" >&2
    exit 2
fi
repo_dir="$(git rev-parse --show-toplevel)"
case "$(cd "$(dirname "${denylist}")" && pwd)/" in
    "${repo_dir}/"*)
        echo "public-safety-scan: the denylist must live outside the repository" >&2
        exit 2
        ;;
esac

mode="${1:---tree}"
work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT
text="${work}/text"

case "${mode}" in
    --tree)
        git -C "${repo_dir}" ls-files -z | while IFS= read -r -d '' file; do
            if [[ -f "${repo_dir}/${file}" ]] && grep -Iq . "${repo_dir}/${file}" 2>/dev/null; then
                awk -v f="${file}" '{ print f ":" FNR ": " $0 }' "${repo_dir}/${file}"
            fi
            printf '%s:0: %s\n' "${file}" "${file}"
        done > "${text}"
        ;;
    --staged)
        git -C "${repo_dir}" diff --cached --unified=0 --no-color > "${text}"
        git -C "${repo_dir}" diff --cached --name-only >> "${text}"
        ;;
    --message)
        cp "${2:?--message needs a file}" "${text}"
        ;;
    --history)
        git -C "${repo_dir}" log --all --no-color -p --format='commit %H%n%an%n%B' > "${text}"
        git -C "${repo_dir}" for-each-ref --format='%(refname)' >> "${text}"
        ;;
    --stdin)
        cat > "${text}"
        ;;
    *)
        echo "public-safety-scan: unknown mode ${mode}" >&2
        exit 2
        ;;
esac

found=0
number=0
while IFS= read -r pattern || [[ -n "${pattern}" ]]; do
    number=$((number + 1))
    [[ -z "${pattern// /}" || "${pattern}" == \#* ]] && continue
    if matches="$(grep -inE -- "${pattern}" "${text}")"; then
        found=1
        while IFS= read -r hit; do
            line_number="${hit%%:*}"
            rest="${hit#*:}"
            if [[ "${mode}" == "--tree" ]]; then
                where="${rest%%: *}"
            else
                where="${mode#--} text line ${line_number}"
            fi
            echo "public-safety: denylist line ${number} matched at ${where}"
        done <<< "${matches}"
    fi
done < "${denylist}"

if (( found )); then
    echo "public-safety-scan: FAILED (${mode}); remove the matched content before it is committed or published" >&2
    exit 1
fi
echo "public-safety-scan: clean (${mode})"
