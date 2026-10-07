//! `rust-qa` command line.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::{Parser, Subcommand};
use rust_qa::config::Config;
use rust_qa::date::Date;
use rust_qa::defences::{dylint_lints, enumerate};
use rust_qa::docs::Resolver;
use rust_qa::finding::Finding;
use rust_qa::pipeline::{Outcome, execute};
use rust_qa::policy::{RecordPolicy, Status, gate, remove_exceptions, set_high_water};
use rust_qa::record::{Record, normalise_rule};
use rust_qa::tools::{Context, RealExecutor, Tool, render_summary, select};
use rust_qa::{DBF_POINTER, Error, builtin, summary, suppression};

/// One QA entry point for a Rust workspace: format, detectors, then runners.
#[derive(Debug, Parser)]
#[command(name = "rust-qa", version)]
struct Cli {
    /// Workspace root (default: the enclosing Cargo workspace).
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    /// Configuration file relative to the root (default: qa/rust-qa.toml when present).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Run the gate: format, detectors, runners. A detector failure stops before any runner.
    Run {
        /// Run only these tools (repeatable); see `rust-qa tools`.
        #[arg(short = 't', long = "tool")]
        tools: Vec<String>,
        /// Limit to these files or directories (repeatable).
        #[arg(short = 'p', long = "path")]
        paths: Vec<PathBuf>,
        /// CI mode: autofix, then fail if the working tree changed (default when `CI` is set).
        #[arg(long)]
        ci: bool,
        /// Never modify files (formatting checks only).
        #[arg(long)]
        no_fix: bool,
        /// Stop at the first failing tool.
        #[arg(long)]
        fail_fast: bool,
    },
    /// List the tool catalogue with phases and whether each is enabled.
    Tools,
    /// List every active defence and the project record, without running anything.
    Rules {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Print the documentation for a rule identifier (offline).
    RuleDoc {
        /// Identifier exactly as printed by a defence.
        id: String,
    },
    /// Fail if any listed defence identifier has no documentation.
    DocsCheck,
    /// Project record commands.
    Record {
        #[command(subcommand)]
        action: RecordAction,
    },
    /// Regenerate the agent summary region (or check it with --check).
    Summary {
        /// Fail instead of writing when the region is stale.
        #[arg(long)]
        check: bool,
    },
}

#[derive(Debug, Subcommand)]
enum RecordAction {
    /// Validate every exception and the record policy (budgets, deadlines, trust, ratchet).
    Check,
    /// List exceptions with the budget status.
    List,
    /// Remove exceptions that match no suppression and lower the ratchet.
    Prune {
        /// Print what would be removed without changing the file.
        #[arg(long)]
        dry_run: bool,
    },
    /// Show the exceptions for a rule id (or a 1-based entry number) and the sites each covers.
    Explain {
        /// Rule id as printed, or an entry number.
        target: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("rust-qa: {error}");
            ExitCode::from(2)
        }
    }
}

fn dispatch(cli: Cli) -> Result<bool, Error> {
    let root = match cli.root {
        Some(root) => root,
        None => discover_root()?,
    };
    let root = std::fs::canonicalize(&root).map_err(|source| Error::Read { path: root, source })?;
    let config = Config::load(&root, cli.config.as_deref())?;
    match cli.command {
        Cmd::Run {
            tools,
            paths,
            ci,
            no_fix,
            fail_fast,
        } => {
            let ci =
                ci || std::env::var("CI").is_ok_and(|v| !v.is_empty() && v != "false" && v != "0");
            run(&root, config, &tools, &paths, ci, !no_fix, fail_fast)
        }
        Cmd::Tools => {
            for tool in Tool::ALL {
                let state = if tool.enabled(&config) {
                    "enabled"
                } else {
                    "disabled"
                };
                println!("{:<12} {:<10} {state}", tool.name(), tool.phase().name());
            }
            Ok(true)
        }
        Cmd::Rules { json } => rules(&root, &config, json),
        Cmd::RuleDoc { id } => rule_doc(&root, &config, &id),
        Cmd::DocsCheck => docs_check(&root, &config),
        Cmd::Record { action } => record_command(&root, &config, action),
        Cmd::Summary { check } => summary_command(&root, &config, check),
    }
}

fn record_command(root: &Path, config: &Config, action: RecordAction) -> Result<bool, Error> {
    let policy = &config.record;
    let file = root.join(&policy.path);
    let record = Record::load(&file)?;
    match action {
        RecordAction::Check => {
            let report = gate(root, policy, Date::today(), false)?;
            println!("{}", report.status);
            for note in &report.notes {
                println!("note: {note}");
            }
            print_findings(&report.findings);
            Ok(report.findings.is_empty())
        }
        RecordAction::List => {
            print_record(&record, policy);
            Ok(true)
        }
        RecordAction::Prune { dry_run } => {
            let exclude = rust_qa::glob_set(&config.scan_exclude)?;
            let scan = suppression::scan(root, &[], &exclude)?;
            let stale = suppression::stale(&scan, &record);
            for index in &stale {
                if let Some(exception) = record.exceptions.get(*index) {
                    println!(
                        "stale: exception #{} `{}` at `{}`",
                        index + 1,
                        exception.rule,
                        exception.path
                    );
                }
            }
            if stale.is_empty() {
                println!("no stale exceptions");
                return Ok(true);
            }
            if dry_run {
                return Ok(true);
            }
            let text = remove_exceptions(&rust_qa::read_to_string(&file)?, &stale);
            let remaining = record.exceptions.len().saturating_sub(stale.len());
            let text = set_high_water(&text, remaining).unwrap_or(text);
            std::fs::write(&file, text).map_err(|source| Error::Write {
                path: file.clone(),
                source,
            })?;
            println!(
                "removed {} stale exception(s); {remaining} remain",
                stale.len()
            );
            Ok(true)
        }
        RecordAction::Explain { target } => explain(root, config, &record, &target),
    }
}

fn explain(root: &Path, config: &Config, record: &Record, target: &str) -> Result<bool, Error> {
    let policy = &config.record;
    let wanted = normalise_rule(target);
    let chosen: Vec<usize> = match target.parse::<usize>() {
        Ok(number) => vec![number.saturating_sub(1)],
        Err(_) => (0..record.exceptions.len())
            .filter(|i| {
                record
                    .exceptions
                    .get(*i)
                    .is_some_and(|e| normalise_rule(&e.rule) == wanted)
            })
            .collect(),
    };
    let chosen: Vec<(usize, &rust_qa::record::Exception)> = chosen
        .into_iter()
        .filter_map(|i| record.exceptions.get(i).map(|e| (i, e)))
        .collect();
    if chosen.is_empty() {
        println!("no exception matches `{target}`");
        return Ok(false);
    }
    let exclude = rust_qa::glob_set(&config.scan_exclude)?;
    let scan = suppression::scan(root, &[], &exclude)?;
    let today = Date::today();
    for (index, exception) in chosen {
        let model = exception.added_by_model.as_deref().unwrap_or("(missing)");
        println!(
            "exception #{} `{}` at `{}`",
            index + 1,
            exception.rule,
            exception.path
        );
        println!("  justification: {}", exception.justification);
        println!(
            "  decided_by: {}; added_by_model: {model} (tier {}); policy-level: {}",
            exception.decided_by.as_deref().unwrap_or("(missing)"),
            policy.tier(model).name(),
            exception.policy
        );
        match (exception.decided_on, exception.review_by) {
            (Some(on), Some(by)) => println!(
                "  decided_on: {on}; review_by: {by} ({} day(s) left)",
                today.days_until(by)
            ),
            _ => println!("  decided_on or review_by missing"),
        }
        let sites: Vec<String> = scan
            .suppressions
            .iter()
            .filter(|s| record.find(&s.rule, &s.path) == Some(index))
            .map(|s| format!("{}:{} ({})", s.path.display(), s.line, s.route.describe()))
            .collect();
        if sites.is_empty() {
            println!("  covers: nothing (stale; `rust-qa record prune` removes it)");
        }
        for site in sites {
            println!("  covers: {site}");
        }
    }
    Ok(true)
}

fn rule_doc(root: &Path, config: &Config, id: &str) -> Result<bool, Error> {
    let resolver = Resolver::new(root, &config.docs_dir);
    let Some(doc) = resolver.resolve(id)? else {
        println!("no documentation for `{id}` (dangling identifier)");
        return Ok(false);
    };
    println!("{}", doc.text);
    eprintln!("(source: {:?})", doc.source);
    Ok(true)
}

fn summary_command(root: &Path, config: &Config, check: bool) -> Result<bool, Error> {
    let Some(file) = config.summary.file.clone() else {
        return Err(Error::Usage(
            "no [summary] file configured in qa/rust-qa.toml".to_owned(),
        ));
    };
    let path = root.join(&file);
    let existing = if path.exists() {
        rust_qa::read_to_string(&path)?
    } else {
        String::new()
    };
    let updated = summary::splice(&existing, &render_summary(root, config, dylint_lints)?);
    if updated == existing {
        return Ok(true);
    }
    if check {
        print_findings(&[Finding {
            rule: builtin::SUMMARY_STALE,
            path: file,
            line: 0,
            message: "generated defence summary is out of date; run `rust-qa summary`".to_owned(),
        }]);
        return Ok(false);
    }
    std::fs::write(&path, updated).map_err(|source| Error::Write { path, source })?;
    Ok(true)
}

fn discover_root() -> Result<PathBuf, Error> {
    let output = Command::new("cargo")
        .args(["locate-project", "--workspace", "--message-format", "plain"])
        .output()
        .map_err(|e| Error::Command {
            command: "cargo locate-project".to_owned(),
            message: e.to_string(),
        })?;
    let manifest = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    match manifest.parent() {
        Some(dir) if output.status.success() => Ok(dir.to_path_buf()),
        _ => std::env::current_dir().map_err(|source| Error::Read {
            path: PathBuf::from("."),
            source,
        }),
    }
}

fn run(
    root: &Path,
    config: Config,
    tools: &[String],
    paths: &[PathBuf],
    ci: bool,
    fix: bool,
    fail_fast: bool,
) -> Result<bool, Error> {
    let selected = select(tools, &config)?;
    let scope = paths
        .iter()
        .map(|path| relative_to_root(root, path))
        .collect::<Result<Vec<_>, _>>()?;
    let ctx = Context::discover(root, config, scope, ci, fix)?;
    let mut executor = RealExecutor {
        ctx,
        dylint_lister: dylint_lints,
    };
    let report = execute(&selected, &mut executor, fail_fast);
    println!("\nrust-qa summary:");
    for (tool, outcome) in &report.results {
        let text = match outcome {
            Outcome::Passed => "passed".to_owned(),
            Outcome::Failed => "FAILED".to_owned(),
            Outcome::Skipped(reason) => format!("skipped ({reason})"),
        };
        println!("  {:<10} {:<12} {text}", tool.phase().name(), tool.name());
    }
    if report.stopped_before_runners {
        println!("detectors failed: runners were not started");
    }
    if !report.passed {
        println!("\n{DBF_POINTER}");
    }
    Ok(report.passed)
}

fn relative_to_root(root: &Path, path: &Path) -> Result<PathBuf, Error> {
    let absolute = std::fs::canonicalize(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })?;
    absolute
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .map_err(|_| {
            Error::Usage(format!(
                "{} is outside the workspace {}",
                path.display(),
                root.display()
            ))
        })
}

fn rules(root: &Path, config: &Config, json: bool) -> Result<bool, Error> {
    let defences = enumerate(root, config, &dylint_lints)?;
    let policy = &config.record;
    let record = Record::load(&root.join(&policy.path))?;
    if json {
        let status = Status::of(&record, policy);
        let value = serde_json::json!({
            "defences": defences.iter().map(|d| serde_json::json!({
                "id": d.id, "engine": d.engine.name(), "level": d.level, "origin": d.origin,
                "doc": format!("rust-qa rule-doc {}", d.id),
            })).collect::<Vec<_>>(),
            "record": record.exceptions.iter().map(|e| serde_json::json!({
                "rule": e.rule, "path": e.path, "justification": e.justification,
                "decided_by": e.decided_by, "added_by_model": e.added_by_model,
                "tier": e.added_by_model.as_deref().map(|m| policy.tier(m).name()),
                "decided_on": e.decided_on.map(|d| d.to_string()),
                "review_by": e.review_by.map(|d| d.to_string()), "policy": e.policy,
            })).collect::<Vec<_>>(),
            "budget": {
                "total": status.total, "max_total": status.max_total,
                "high_water": status.high_water, "max_per_rule": status.max_per_rule,
                "policy": status.policy, "max_policy": status.max_policy,
                "tiers": status.tiers.iter().map(|(tier, count, cap)| serde_json::json!({
                    "tier": tier.name(), "count": count, "max_count": cap,
                })).collect::<Vec<_>>(),
            },
        });
        println!("{value:#}");
        return Ok(true);
    }
    println!("Active defences (derived from configuration; resolve with `rust-qa rule-doc <id>`):");
    for defence in &defences {
        println!(
            "  {:<40} {:<9} {:<36} {}",
            defence.id,
            defence.engine.name(),
            defence.level,
            defence.origin
        );
    }
    println!();
    print_record(&record, policy);
    Ok(true)
}

fn print_record(record: &Record, policy: &RecordPolicy) {
    println!("Project record ({} exception(s)):", record.exceptions.len());
    for exception in &record.exceptions {
        let review_by = exception
            .review_by
            .map_or_else(|| "-".to_owned(), |date| date.to_string());
        println!(
            "  {:<40} {:<36} review by {review_by:<10} {}",
            exception.rule, exception.path, exception.justification
        );
    }
    println!("{}", Status::of(record, policy));
}

fn docs_check(root: &Path, config: &Config) -> Result<bool, Error> {
    let (ids, dangling) = rust_qa::docs::dangling(root, config, &dylint_lints)?;
    for id in &dangling {
        println!(
            "dangling identifier: `{id}` has no documentation (add {})",
            config
                .docs_dir
                .join(rust_qa::docs::doc_file_name(id))
                .display()
        );
    }
    println!(
        "{} identifier(s) checked, {} dangling",
        ids.len(),
        dangling.len()
    );
    Ok(dangling.is_empty())
}

fn print_findings(findings: &[Finding]) {
    for finding in findings {
        println!("{finding}");
    }
    if !findings.is_empty() {
        println!("\n{DBF_POINTER}");
    }
}
