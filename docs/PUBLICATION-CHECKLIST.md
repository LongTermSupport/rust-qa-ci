# Pre-publication audit

Run every step before changing this repository's visibility to public. Commit history, tags, issue
and pull-request text (including edit history) and CI logs all become public with the repository.

The denylist is supplied from outside the repository (`RQACI_DENYLIST=/path/to/denylist` or
`git config rqaci.denylist /path/to/denylist`); it is never committed here. Start from a fresh
clone with every branch and tag fetched (`git fetch --all --tags`).

1. Working tree: `scripts/public-safety-scan.sh --tree` and `scripts/secret-scan.sh --tree` report
   clean.
2. History: `scripts/public-safety-scan.sh --history` and `scripts/secret-scan.sh --history` report
   clean (every diff, commit message, author and committer identity, annotated tag message and ref
   name). Compare `git ls-remote origin` with the local refs so no remote-only branch is missed.
3. Identities: list `git log --all --format='%an <%ae>%n%cn <%ce>'` and every tagger; confirm each
   address is one its owner is content to publish (a forge no-reply address is the usual choice).
4. Issues and pull requests: export every title, body and comment
   (`gh issue list --state all --json title,body,comments`, `gh pr list --state all --json
   title,body,comments`) and pipe the text through `scripts/public-safety-scan.sh --stdin`. Edited
   text keeps its earlier versions; review edit history on anything that was ever changed.
5. Releases: `gh release list`; scan release titles and notes the same way.
6. CI: review workflow logs and artefacts; delete runs whose logs carry environment-specific detail.
7. Fixtures and examples: confirm every example uses neutral names, `example.com` and documentation
   address ranges (RFC 5737, RFC 3849).
8. Settings: no deploy keys, secrets other than `RQACI_DENYLIST`, environments, webhooks, topics or
   description naming private systems.
9. Licence: a `LICENSE` file is present and the crates' `license` field matches it.
10. Record the audit (date, commit hash, who ran it) in the release notes of the first public tag.
