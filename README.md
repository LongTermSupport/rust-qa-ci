# rust-qa-ci

One QA entry point for Rust workspaces, built for the Defence Before Fix method
(https://defence-before-fix.github.io/): bespoke rules on rustc's type information, blocking
enforcement through a single command, offline rule documentation, a project record for every
exception, and an enumerable list of active defences.

Status: early (0.x). Conformance is **partial**; see [docs/CONFORMANCE.md](docs/CONFORMANCE.md).
Licence: MIT ([LICENSE](LICENSE)). Unless you state otherwise, any contribution you intentionally
submit for inclusion is licensed under the same terms.

## What it runs

`rust-qa run` executes three phases in a fixed order. Every detector runs so one invocation reports
all static findings; any format or detector failure stops the run before the first runner.

| Phase | Tool (`-t` name) | What |
|---|---|---|
| format | `fmt` | `cargo fmt` (fixes locally; `--no-fix` checks) |
| detectors | `record` | project record validation and anti-bloat policy (`rqaci.record.invalid`, `.budget`, `.ratchet`, `.expired`) |
| | `suppression` | unrecorded suppressions and `allow` attributes (`rqaci.suppression.unrecorded`, `rqaci.suppression.allow`, `rqaci.record.stale`) |
| | `summary` | agent summary freshness (`rqaci.summary.stale`) |
| | `docs` | every listed identifier resolves to documentation (`rqaci.docs.dangling`) |
| | `ast-grep` | supplementary syntactic rules: the project's `sgconfig.yml` rules, then the bundled `rqaci.ast.*` pack |
| | `clippy` | Clippy with `-D warnings` and the Cargo `[lints]` tables |
| | `dylint` | type-aware rule libraries from `[workspace.metadata.dylint]` |
| runners | `deny` | cargo-deny (advisories, bans, licences, sources) |
| | `audit` | cargo-audit (off by default) |
| | `machete` | unused dependencies |
| | `test` | cargo-nextest, then doctests |
| | `coverage` | cargo-llvm-cov with a line threshold |
| | `mutants` | cargo-mutants (`off`, `diff` against a base, or `full`) |

Options: `-t <tool>` (repeatable), `-p <path>` (repeatable; one file is enough; per-crate engines
analyse the owning crate and print only diagnostics inside the path), `--ci` (also implied by a
non-empty `CI` variable: autofix, then fail if the working tree changed), `--no-fix`, `--fail-fast`.

Other commands, none of which run a defence:

- `rust-qa rules [--json]`: every active defence, derived from configuration, plus the project record
- `rust-qa rule-doc <id>`: documentation for any printed identifier, offline
- `rust-qa docs-check`: fail if a listed identifier has no documentation
- `rust-qa record check|list`: validate (structure and policy) or list project-record exceptions,
  with the budget status
- `rust-qa record prune [--dry-run]`: delete exceptions that cover no suppression and lower the
  ratchet
- `rust-qa record explain <rule|number>`: provenance, trust tier, review deadline and the sites an
  exception covers
- `rust-qa summary [--check]`: regenerate (or check) the agent summary region
- `rust-qa tools`: the tool catalogue

## Writing rules

Write each rule against the richest layer that can express it.

1. **dylint (primary)**: late lint passes with full rustc type information (`LateContext`,
   `typeck_results`, trait queries). Create a library with `cargo dylint new`, keep its own pinned
   nightly `rust-toolchain.toml` (isolated from the product toolchain), prove it with
   `dylint_testing` UI tests, and list it under `[workspace.metadata.dylint] libraries`. Declare
   lints at `Deny` and choose stable, namespaced names.
2. **ast-grep (supplementary)**: YAML rules for cheap single-file checks where syntax provably
   suffices. Set `severity: error` to block. Record in the rule's doc why syntax suffices. List
   rule directories under `ruleDirs` in `sgconfig.yml` at the workspace root.

`rust-qa` also ships a small ast-grep pack, on by default, that runs beside the project's rules
(a second `ast-grep scan`, so neither set hides the other's findings; it needs no `sgconfig.yml`
and honours `scan_exclude`). Each rule is a cheap single-file check that no Clippy or rustc lint
covers, and its doc says why syntax suffices:

| Rule | Flags |
|---|---|
| `rqaci.ast.shell-c-command` | `Command::new("sh")` (or another shell) given `-c` and a command string |
| `rqaci.ast.hardcoded-temp-path` | string literals under `/tmp` or `/var/tmp` |
| `rqaci.ast.tls-verification-disabled` | `danger_accept_invalid_certs(true)` / `danger_accept_invalid_hostnames(true)` |

Switch the pack off with `[ast_grep] bundled = false`; project rules keep running. Silence one
finding with `// ast-grep-ignore: <id>` plus a project-record exception for `ast-grep::<id>`.
Keep project rule ids out of the `rqaci.` namespace.

Every rule identifier needs a document `<docs_dir>/<id>.md` (`::` becomes `.`) whose sections say what
it flags, why, and how to fix correctly. `rust-qa docs-check` enforces this.

## Project record

`qa/record.toml` holds every exception; nothing else can silence a defence:

```toml
[ratchet]                                 # signed ceiling on the number of exceptions
high_water = 1
decided_by = "alice"                      # one of [record] deciders
decided_on = 2026-01-15
reason = "Why the record holds this many entries."

[[exception]]
rule = "clippy::needless_pass_by_value"   # identifier exactly as printed
path = "src/handlers/upload.rs"           # one file; a glob needs `policy = true`
justification = "The framework requires owned extractors; borrowing does not compile here."
decided_by = "alice"                      # who decided; a configured decider for `human` entries
added_by_model = "human"                  # or the id of the model that wrote the entry
decided_on = 2026-01-15                   # UTC date
review_by = 2026-04-15                    # UTC date; within the tier's horizon of decided_on
# policy = true                           # deliberate broad (glob) exception, separately capped
```

Empty, generic ("TODO", "legacy", "needed for now", ...) or very short justifications are rejected.
Exceptions that match nothing fail the gate as stale (`rust-qa record prune` deletes them).

Prefer an in-source `#[expect(lint, reason = "...")]`: rustc fails it as soon as the lint stops
firing, so the exception removes itself. `#[allow]` fails `rqaci.suppression.allow` even when
recorded.

The `suppression` detector reads source text. A lint attribute that a workspace macro assembles
from fragments (`#[$level(lint)]`, a caller-supplied lint name, a workspace proc macro) only
exists after expansion; the bundled dylint lint `rqaci_suppression_generated` reports those that
cover caller-written code, and its own suppression is a literal, recorded `expect`, so every
route ends in the record.

### Anti-bloat controls

Exception records (baselines, allowlists) grow by default: each entry is cheap to add and nobody
owns its removal. The `record` detector makes growth expensive and shrinkage automatic:

| Control | Rule | Fails when |
|---|---|---|
| Budget | `rqaci.record.budget` | more than `max_total` entries, `max_per_rule` for one rule id, `max_policy` broad entries, or a tier's `max_count` |
| Breadth | `rqaci.record.invalid` | a glob path without `policy = true` (or the marker on a single file) |
| Provenance | `rqaci.record.invalid` | `decided_by`, `added_by_model`, `decided_on` or `review_by` missing; a model that maps to no trust tier; `human` claimed by someone not in `deciders` |
| Horizon | `rqaci.record.invalid` | `review_by` further from `decided_on` than the tier allows |
| Co-sign | `rqaci.record.invalid` | a rule in a `[[record.trust.cosign]]` category written below its `min_tier` without a configured decider as `decided_by` |
| Expiry | `rqaci.record.expired` | `review_by` is in the past |
| Ratchet | `rqaci.record.ratchet` | no `[ratchet]`; more entries than `high_water`; an unsigned or generic ceiling; a raise without a fresh signature; an unlowered ceiling in CI |

The ratchet: when entries are removed, a local `rust-qa run` (or `record prune`) rewrites
`high_water` to the new count, and CI fails until that is committed. Adding an entry needs
`high_water` raised in the same change, signed by a configured decider with today's `decided_on`
and a reason. The gate compares the ceiling with the record at `HEAD` and at the merge base with
`[record] history_base`; a raise must carry a `decided_on` newer than the previous signature and
at most 14 days old.

Trust tiers: `added_by_model = "human"` is the `human` tier; other ids map to `high`, `medium` or
`low` by the longest matching prefix in `[record.trust] models`; an unmatched id fails. Lower tiers
must have equal or stricter review horizons and count caps (the configuration is rejected
otherwise). `rust-qa record list`, `rust-qa rules`, the gate output and the summary region print
the count against each budget, the count per tier, and the soonest review and oldest decision.

Trade-offs, stated plainly:

- Anything in the repository can be edited by whoever can commit, including an agent writing a
  person's name into `decided_by`. The controls make growth explicit, attributed and visible in
  review; they do not authenticate. Protect `qa/record.toml` and `qa/rust-qa.toml` with code owners
  and branch protection if signatures must be trusted.
- Raise detection needs git history. Without it (no repository, the file is new, the base ref is
  missing) the gate prints a note and checks only the history-free rules; the ratchet still blocks
  growth past the committed `high_water`.
- A direct push to the base branch has no merge base before the change, so the comparison is with
  `HEAD`: run the gate before committing (pre-commit or the local gate) to catch an unsigned raise.
- Budgets are counts, not weights: one broad entry counts once. Breadth is controlled separately
  through `policy = true` and `max_policy`.
- Expiry uses the system clock (UTC). An expired entry blocks unrelated work until someone
  re-decides or removes it; that friction is the point.

## Configuration

`qa/rust-qa.toml` (optional; unknown keys are rejected). Defaults:

```toml
record = "qa/record.toml"    # shorthand for [record] path, with the defaults below
docs_dir = "docs/defences"
scan_exclude = []            # globs excluded from the suppression scan and the bundled ast-grep
                             # pack; target/ and .git/ are always skipped

[summary]
# file = "AGENTS.md"         # agent-loaded file that receives the generated region; unset = off

[tools]                      # explicit `-t` requests override these switches
fmt = true
record = true
suppression = true
summary = true
docs = true
ast_grep = true
clippy = true
dylint = true
deny = true
audit = false
machete = true
test = true
coverage = true
mutants = true

[ast_grep]
bundled = true               # run the bundled rqaci.ast.* pack beside the project's own rules

[coverage]
fail_under_lines = 80

[mutants]
mode = "off"                 # off | diff | full
base = "origin/main"         # diff mode compares against the merge base with this ref
```

The record policy is a `[record]` table instead of the `record = "..."` shorthand:

```toml
[record]
path = "qa/record.toml"
max_total = 10               # entries in the record (also caps [ratchet] high_water)
max_per_rule = 3             # entries for one rule id
max_policy = 0               # broad entries marked policy = true; at most max_per_rule
max_review_days = 180        # longest decided_on -> review_by gap for any tier
deciders = []                # people who sign the ratchet, `human` entries and co-signs;
                             # automation-like ids (bot, ci, agent, model names) are rejected
history_base = "origin/main" # merge base for raise detection, besides HEAD

[record.trust]
models = {}                  # model id prefix -> high | medium | low, for example
                             # { opus = "high", sonnet = "medium", haiku = "low" }

[record.trust.human]         # each tier: max_review_days and max_count; unset = record-wide cap
[record.trust.high]
max_review_days = 90
[record.trust.medium]
max_review_days = 30
max_count = 3
[record.trust.low]
max_review_days = 14
max_count = 1

# [[record.trust.cosign]]    # rule categories that need a configured decider below min_tier
# rules = ["cargo-deny::*", "my_secret_*"]
# min_tier = "high"
```

A tool whose own configuration is absent (no dylint metadata, no `deny.toml`; for ast-grep, no
`sgconfig.yml` and the bundled pack off) reports itself as skipped.

## Installing

Pin an exact tag. Install the binary and the pinned external tools:

```bash
cargo install --locked --git https://github.com/LongTermSupport/rust-qa-ci --tag v0.3.0 rust-qa
git clone --depth 1 --branch v0.3.0 https://github.com/LongTermSupport/rust-qa-ci /tmp/rust-qa-ci
/tmp/rust-qa-ci/scripts/install-tools.sh            # versions from tools.lock
```

Bundled type-aware rules load from the same tag:

```toml
[workspace.metadata.dylint]
libraries = [
    { git = "https://github.com/LongTermSupport/rust-qa-ci", tag = "v0.3.0", pattern = "lints/rqaci_lints" },
]
```

## Public-safety guard

This repository must stay safe to publish. Two scans guard it:

- `scripts/public-safety-scan.sh` checks the tree, staged changes, commit messages, the whole history
  (diffs, messages, identities, tag messages, ref names) or arbitrary text against a denylist
  supplied from outside the repository (`RQACI_DENYLIST=/path` or `git config rqaci.denylist /path`).
  The list belongs to whoever maintains a private consumer; the format is in
  [docs/denylist.example](docs/denylist.example). Findings print a line number, never the term.
- `scripts/secret-scan.sh` runs gitleaks over the history, staged changes or tree, with no
  suppression route.

Enable the hooks with `git config core.hooksPath .githooks`: pre-commit scans staged changes,
commit-msg the message, pre-push the whole history. CI (`.github/workflows/public-safety.yml`) runs
both scans on every push and pull request, weekly, and on maintainers' issue, pull request and
comment text; it reads the denylist from the `RQACI_DENYLIST` repository secret and fails if the
secret is missing. The pre-publication audit is
[docs/PUBLICATION-CHECKLIST.md](docs/PUBLICATION-CHECKLIST.md).
