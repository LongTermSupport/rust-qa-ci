# Pre-publication audit

Run every step before changing this repository's visibility to public. Commit history, issue and
pull-request text (including edit history) and CI logs all become public with the repository.

The denylist is supplied from outside the repository (`RQACI_DENYLIST=/path/to/denylist`); it is
never committed here.

1. Working tree: `scripts/public-safety-scan.sh --tree` reports clean.
2. History: `scripts/public-safety-scan.sh --history` reports clean (every diff, commit message,
   author line and ref name).
3. Issues and pull requests: export every title, body and comment
   (`gh issue list --state all --json title,body,comments`, `gh pr list --state all --json
   title,body,comments`) and pipe the text through `scripts/public-safety-scan.sh --stdin`.
4. CI: review workflow logs and artefacts; delete runs whose logs carry environment-specific detail.
5. Fixtures and examples: confirm every example uses neutral names, `example.com` and documentation
   address ranges (RFC 5737, RFC 3849).
6. Settings: no deploy keys, secrets or environments named after private systems.
7. Record the audit (date, commit hash, who ran it) in the release notes of the first public tag.
