//! Tool catalogue, phases, selection and planned command lines.
#![cfg(test)]

use std::path::PathBuf;

use rust_qa::config::{Config, MutantsMode};
use rust_qa::pipeline::Phase;
use rust_qa::tools::{Context, Tool, owning_packages, plan, select};

#[test]
fn catalogue_is_ordered_format_then_detectors_then_runners() {
    let phases: Vec<Phase> = Tool::ALL.iter().map(|t| t.phase()).collect();
    let mut sorted = phases.clone();
    sorted.sort();
    assert_eq!(phases, sorted);
    assert_eq!(Tool::ALL.first().map(|t| t.phase()), Some(Phase::Format));
    assert_eq!(Tool::Clippy.phase(), Phase::Detectors);
    assert_eq!(Tool::Dylint.phase(), Phase::Detectors);
    assert_eq!(Tool::Deny.phase(), Phase::Runners);
    assert_eq!(Tool::Test.phase(), Phase::Runners);
}

#[test]
fn names_round_trip() {
    for tool in Tool::ALL {
        assert_eq!(Tool::from_name(tool.name()), Some(tool));
    }
    assert_eq!(Tool::from_name("ast-grep"), Some(Tool::AstGrep));
    assert_eq!(Tool::from_name("nope"), None);
}

#[test]
fn selection_is_canonical_order_regardless_of_request_order() {
    let config = Config::default();
    let picked = select(
        &["test".to_owned(), "clippy".to_owned(), "fmt".to_owned()],
        &config,
    )
    .unwrap();
    assert_eq!(picked, vec![Tool::Fmt, Tool::Clippy, Tool::Test]);
}

#[test]
fn default_selection_is_every_enabled_tool() {
    let mut config = Config::default();
    config.tools.coverage = false;
    let picked = select(&[], &config).unwrap();
    assert!(!picked.contains(&Tool::Audit), "audit is off by default");
    assert!(!picked.contains(&Tool::Coverage));
    assert!(picked.contains(&Tool::Dylint));
}

#[test]
fn unknown_tool_names_are_rejected() {
    assert!(select(&["clipy".to_owned()], &Config::default()).is_err());
}

fn context(config: Config) -> Context {
    Context {
        root: PathBuf::from("/ws"),
        config,
        scope: Vec::new(),
        packages: Vec::new(),
        doctests: true,
        ci: false,
        fix: true,
    }
}

#[test]
fn owning_packages_are_found_from_cargo_metadata() {
    let metadata = serde_json::json!({
        "packages": [
            {"name": "a", "manifest_path": "/ws/crates/a/Cargo.toml", "targets": [{"kind": ["lib"]}]},
            {"name": "b", "manifest_path": "/ws/crates/b/Cargo.toml", "targets": [{"kind": ["bin"]}]},
            {"name": "app", "manifest_path": "/ws/apps/app/Cargo.toml", "targets": [{"kind": ["bin"]}]}
        ]
    });
    let root = std::path::Path::new("/ws");
    let file = owning_packages(&metadata, root, &[PathBuf::from("crates/a/src/lib.rs")]);
    assert_eq!(file, (vec!["a".to_owned()], true));
    let dir = owning_packages(&metadata, root, &[PathBuf::from("crates")]);
    assert_eq!(dir, (vec!["a".to_owned(), "b".to_owned()], true));
    let bin = owning_packages(&metadata, root, &[PathBuf::from("apps/app/src/main.rs")]);
    assert_eq!(bin, (vec!["app".to_owned()], false));
}

#[test]
fn doctests_are_skipped_without_library_targets() {
    let mut ctx = context(Config::default());
    ctx.doctests = false;
    assert_eq!(
        line(Tool::Test, &ctx),
        vec!["cargo nextest run --workspace --all-features"]
    );
}

fn line(tool: Tool, ctx: &Context) -> Vec<String> {
    plan(tool, ctx)
        .into_iter()
        .map(|c| format!("{} {}", c.program, c.args.join(" ")))
        .collect()
}

#[test]
fn clippy_denies_warnings_across_all_targets() {
    let ctx = context(Config::default());
    assert_eq!(
        line(Tool::Clippy, &ctx),
        vec!["cargo clippy --workspace --all-targets --all-features -- -D warnings"]
    );
}

#[test]
fn scoped_clippy_targets_owning_packages_and_requests_json() {
    let mut ctx = context(Config::default());
    ctx.scope = vec![PathBuf::from("crates/a/src/lib.rs")];
    ctx.packages = vec!["a".to_owned()];
    assert_eq!(
        line(Tool::Clippy, &ctx),
        vec!["cargo clippy -p a --all-targets --all-features --message-format=json -- -D warnings"]
    );
}

#[test]
fn format_fixes_locally_and_checks_when_fixing_is_off() {
    let mut ctx = context(Config::default());
    assert_eq!(line(Tool::Fmt, &ctx), vec!["cargo fmt --all"]);
    ctx.fix = false;
    assert_eq!(line(Tool::Fmt, &ctx), vec!["cargo fmt --all --check"]);
}

#[test]
fn tests_run_nextest_then_doctests() {
    let ctx = context(Config::default());
    assert_eq!(
        line(Tool::Test, &ctx),
        vec![
            "cargo nextest run --workspace --all-features",
            "cargo test --workspace --all-features --doc"
        ]
    );
}

#[test]
fn coverage_gate_uses_configured_threshold() {
    let mut config = Config::default();
    config.coverage.fail_under_lines = 91;
    let ctx = context(config);
    assert_eq!(
        line(Tool::Coverage, &ctx),
        vec!["cargo llvm-cov nextest --workspace --all-features --fail-under-lines 91"]
    );
}

#[test]
fn mutants_follow_the_configured_mode() {
    let mut config = Config::default();
    assert_eq!(
        line(Tool::Mutants, &context(config.clone())),
        Vec::<String>::new()
    );
    config.mutants.mode = MutantsMode::Full;
    assert_eq!(
        line(Tool::Mutants, &context(config.clone())),
        vec!["cargo mutants --workspace"]
    );
    config.mutants.mode = MutantsMode::Diff;
    assert_eq!(
        line(Tool::Mutants, &context(config)),
        vec!["cargo mutants --workspace --in-diff target/rust-qa/mutants.diff"]
    );
}

#[test]
fn ast_grep_scans_the_scope_or_the_whole_tree() {
    let mut ctx = context(Config::default());
    assert_eq!(line(Tool::AstGrep, &ctx), vec!["ast-grep scan"]);
    ctx.scope = vec![PathBuf::from("src/lib.rs")];
    assert_eq!(line(Tool::AstGrep, &ctx), vec!["ast-grep scan src/lib.rs"]);
}

#[test]
fn dylint_loads_every_configured_library() {
    let ctx = context(Config::default());
    let planned = plan(Tool::Dylint, &ctx);
    assert_eq!(
        format!("{} {}", planned[0].program, planned[0].args.join(" ")),
        "cargo dylint --all --workspace -- --all-targets --all-features"
    );
}
