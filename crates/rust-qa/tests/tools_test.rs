//! Tool catalogue, phases, selection and planned command lines.
#![cfg(test)]

use std::path::PathBuf;

use rust_qa::config::{Config, MutantsMode};
use rust_qa::diagnostics::Diagnostic;
use rust_qa::pipeline::{Phase, Step};
use rust_qa::tools::{
    Context, Disposition, Tool, deny_report, diagnostic_text, dylint_disposition, library_flags,
    owning_packages, plan, select,
};

#[test]
fn catalogue_is_ordered_format_then_detectors_then_runners() {
    let phases: Vec<Phase> = Tool::ALL.iter().map(|t| t.phase()).collect();
    let mut sorted = phases.clone();
    sorted.sort();
    assert_eq!(phases, sorted);
    assert_eq!(Tool::ALL.first().map(|t| t.phase()), Some(Phase::Format));
    assert_eq!(Tool::Clippy.phase(), Phase::Detectors);
    assert_eq!(Tool::Dylint.phase(), Phase::Detectors);
    assert_eq!(Tool::Docs.phase(), Phase::Detectors);
    assert!(
        Tool::Docs < Tool::Clippy,
        "docs is checked before the compiler-backed detectors"
    );
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
    assert_eq!(
        picked,
        vec![
            Step::Tool(Tool::Fmt),
            Step::Tool(Tool::Clippy),
            Step::Tool(Tool::Test)
        ]
    );
}

#[test]
fn default_selection_is_every_enabled_tool() {
    let mut config = Config::default();
    config.tools.coverage = false;
    let picked = select(&[], &config).unwrap();
    assert!(
        !picked.contains(&Step::Tool(Tool::Audit)),
        "audit is off by default"
    );
    assert!(!picked.contains(&Step::Tool(Tool::Coverage)));
    assert!(picked.contains(&Step::Tool(Tool::Dylint)));
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
        vec![
            "cargo clippy --workspace --all-targets --all-features --message-format=json -- -D warnings"
        ]
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

fn ast_grep_context(config: Config, sgconfig: bool) -> (tempfile::TempDir, Context) {
    let dir = tempfile::tempdir().unwrap();
    if sgconfig {
        std::fs::write(dir.path().join("sgconfig.yml"), "ruleDirs: []\n").unwrap();
    }
    let mut ctx = context(config);
    ctx.root = dir.path().to_path_buf();
    (dir, ctx)
}

#[test]
fn ast_grep_runs_project_rules_then_the_bundled_pack_over_the_scope_or_the_whole_tree() {
    let (_dir, mut ctx) = ast_grep_context(Config::default(), true);
    assert_eq!(
        line(Tool::AstGrep, &ctx),
        vec![
            "ast-grep scan --json=compact",
            "ast-grep scan --json=compact --rule target/rust-qa/ast-grep/rqaci-pack.yml"
        ]
    );
    ctx.scope = vec![PathBuf::from("src/lib.rs")];
    assert_eq!(
        line(Tool::AstGrep, &ctx),
        vec![
            "ast-grep scan --json=compact src/lib.rs",
            "ast-grep scan --json=compact --rule target/rust-qa/ast-grep/rqaci-pack.yml src/lib.rs"
        ]
    );
}

#[test]
fn ast_grep_bundled_pack_runs_without_a_project_sgconfig_and_honours_scan_exclude() {
    let config = Config {
        scan_exclude: vec!["fixtures/**".to_owned()],
        ..Config::default()
    };
    let (_dir, ctx) = ast_grep_context(config, false);
    assert_eq!(
        line(Tool::AstGrep, &ctx),
        vec![
            "ast-grep scan --json=compact --rule target/rust-qa/ast-grep/rqaci-pack.yml --globs !fixtures/**"
        ]
    );
}

#[test]
fn ast_grep_bundled_pack_switched_off_leaves_only_project_rules() {
    let mut config = Config::default();
    config.ast_grep.bundled = false;
    let (_dir, ctx) = ast_grep_context(config.clone(), true);
    assert_eq!(
        line(Tool::AstGrep, &ctx),
        vec!["ast-grep scan --json=compact"]
    );
    let (_dir, ctx) = ast_grep_context(config, false);
    assert_eq!(line(Tool::AstGrep, &ctx), Vec::<String>::new());
}

#[test]
fn dylint_loads_every_configured_library() {
    let ctx = context(Config::default());
    let planned = plan(Tool::Dylint, &ctx);
    assert_eq!(
        format!("{} {}", planned[0].program, planned[0].args.join(" ")),
        "cargo dylint --all --workspace -- --all-targets --all-features --message-format=json"
    );
}

#[test]
fn tools_do_not_inherit_the_package_environment_of_cargo_run() {
    // cargo sets the same variables for this test binary as `cargo run` sets for rust-qa.
    assert!(std::env::var_os("CARGO_PKG_NAME").is_some());
    let ctx = context(Config::default());
    let command = plan(Tool::Machete, &ctx)[0].command();
    let removed: Vec<String> = command
        .get_envs()
        .filter(|(_, value)| value.is_none())
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    for name in ["CARGO_PKG_NAME", "CARGO_PKG_VERSION", "CARGO_MANIFEST_DIR"] {
        assert!(removed.iter().any(|r| r == name), "{name} is passed on");
    }
    assert!(
        !removed.iter().any(|r| r == "CARGO" || r == "PATH"),
        "only the package description is removed: {removed:?}"
    );
}

#[test]
fn cargo_runs_locked_when_the_workspace_has_a_lock_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.lock"), "").unwrap();
    let mut ctx = context(Config::default());
    ctx.root = dir.path().to_path_buf();
    assert_eq!(
        line(Tool::Clippy, &ctx),
        vec![
            "cargo clippy --workspace --all-targets --all-features --locked --message-format=json -- -D warnings"
        ]
    );
    assert_eq!(
        line(Tool::Dylint, &ctx),
        vec![
            "cargo dylint --all --workspace -- --all-targets --all-features --locked --message-format=json"
        ]
    );
    assert_eq!(
        line(Tool::Test, &ctx),
        vec![
            "cargo nextest run --workspace --all-features --locked",
            "cargo test --workspace --all-features --doc --locked"
        ]
    );
    assert_eq!(
        line(Tool::Coverage, &ctx),
        vec!["cargo llvm-cov nextest --workspace --all-features --locked --fail-under-lines 80"]
    );
}

fn with_workspace() -> (tempfile::TempDir, Context) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\n[workspace.metadata.dylint]\nlibraries = [{ path = \"lints\" }, \
         { git = \"https://example.com/l\", tag = \"v1\", pattern = \"lib\" }]\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("lints")).unwrap();
    std::fs::write(root.join("lints/Cargo.toml"), "[workspace]\n").unwrap();
    std::fs::write(root.join("lints/Cargo.lock"), "").unwrap();
    let config = Config {
        workspaces: vec![PathBuf::from("lints")],
        ..Config::default()
    };
    let mut ctx = context(config);
    ctx.root = root.to_path_buf();
    (dir, ctx)
}

fn located(tool: Tool, ctx: &Context) -> Vec<(Option<PathBuf>, bool, String)> {
    plan(tool, ctx)
        .into_iter()
        .map(|c| {
            (
                c.dir.clone(),
                c.own_toolchain,
                format!("{} {}", c.program, c.args.join(" ")),
            )
        })
        .collect()
}

#[test]
fn a_further_workspace_is_formatted_linted_and_tested_in_its_own_directory_and_toolchain() {
    let (_dir, mut ctx) = with_workspace();
    ctx.fix = false;
    let lints = Some(PathBuf::from("lints"));
    assert_eq!(
        located(Tool::Fmt, &ctx),
        vec![
            (None, false, "cargo fmt --all --check".to_owned()),
            (lints.clone(), true, "cargo fmt --all --check".to_owned())
        ]
    );
    assert_eq!(
        located(Tool::Clippy, &ctx)[1],
        (
            lints.clone(),
            true,
            "cargo clippy --workspace --all-targets --all-features --locked --message-format=json -- -D warnings"
                .to_owned()
        )
    );
    assert_eq!(
        located(Tool::Test, &ctx)[2],
        (
            lints,
            true,
            "cargo test --workspace --all-features --locked".to_owned()
        )
    );
    assert_eq!(
        located(Tool::Coverage, &ctx).len(),
        1,
        "coverage stays with the main workspace"
    );
}

#[test]
fn a_further_workspace_loads_each_dylint_library_of_the_root() {
    let (dir, ctx) = with_workspace();
    let planned = located(Tool::Dylint, &ctx);
    let root = dir.path().display();
    assert_eq!(planned.len(), 3);
    assert_eq!(
        planned[1].2,
        format!(
            "cargo dylint --path {root}/lints --workspace -- --all-targets --all-features --locked --message-format=json"
        )
    );
    assert_eq!(
        planned[2].2,
        "cargo dylint --git https://example.com/l --tag v1 --pattern lib --workspace -- --all-targets --all-features --locked --message-format=json"
    );
}

#[test]
fn library_flags_translate_path_and_git_entries() {
    let entry: toml::Table =
        toml::from_str("git = \"https://example.com/l\"\nrev = \"abc\"\npattern = \"p\"\n")
            .unwrap();
    assert_eq!(
        library_flags(std::path::Path::new("/ws"), &entry).join(" "),
        "--git https://example.com/l --rev abc --pattern p"
    );
}

#[test]
fn a_subset_run_checks_only_the_workspaces_that_hold_a_scoped_path() {
    let (_dir, mut ctx) = with_workspace();
    ctx.scope = vec![PathBuf::from("lints/src/lib.rs")];
    let planned = located(Tool::Clippy, &ctx);
    assert_eq!(planned.len(), 1, "{planned:?}");
    assert_eq!(planned[0].0, Some(PathBuf::from("lints")));
    assert!(planned[0].2.contains("--message-format=json"));
    ctx.scope = vec![PathBuf::from("src/lib.rs")];
    ctx.packages = vec!["a".to_owned()];
    let planned = located(Tool::Clippy, &ctx);
    assert_eq!(planned.len(), 1);
    assert_eq!(planned[0].0, None);
}

#[test]
fn a_command_with_its_own_toolchain_does_not_inherit_the_toolchain_variables() {
    let (_dir, ctx) = with_workspace();
    let cmd = plan(Tool::Fmt, &ctx).pop().unwrap();
    let removed: Vec<String> = cmd
        .command()
        .get_envs()
        .filter(|(_, value)| value.is_none())
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    // cargo sets these for this test binary, as rustup and cargo would for rust-qa.
    assert!(removed.iter().any(|r| r == "CARGO"), "{removed:?}");
    assert!(!removed.iter().any(|r| r == "CARGO_HOME" || r == "PATH"));
}

#[test]
fn ast_grep_runs_the_project_rule_tests_when_sgconfig_declares_them() {
    let (dir, ctx) = ast_grep_context(Config::default(), true);
    std::fs::write(
        dir.path().join("sgconfig.yml"),
        "ruleDirs: [rules]\ntestConfigs:\n  - testDir: rule-tests\n",
    )
    .unwrap();
    assert_eq!(
        line(Tool::AstGrep, &ctx).last().map(String::as_str),
        Some("ast-grep test --skip-snapshot-tests")
    );
}

#[test]
fn stages_are_selected_by_name_and_run_in_their_phase() {
    let config: Config = toml::from_str(
        "[[stage]]\nname = \"tsc\"\nphase = \"detectors\"\nsays = \"s\"\nfix = \"f\"\ncommand = [\"tsc\"]\n",
    )
    .unwrap();
    let picked = select(&["tsc".to_owned(), "fmt".to_owned()], &config).unwrap();
    assert_eq!(
        picked,
        vec![
            Step::Tool(Tool::Fmt),
            Step::Stage {
                index: 0,
                phase: Phase::Detectors
            }
        ]
    );
    assert!(select(&[], &config).unwrap().contains(&Step::Stage {
        index: 0,
        phase: Phase::Detectors
    }));
    let unknown = select(&["nope".to_owned()], &config)
        .unwrap_err()
        .to_string();
    assert!(unknown.contains("tsc"), "{unknown}");
}

fn diagnostic(code: Option<&str>, level: &str, file: Option<&str>) -> Diagnostic {
    Diagnostic {
        code: code.map(ToOwned::to_owned),
        level: level.to_owned(),
        rendered: format!("{level}: something\n"),
        files: file.map(PathBuf::from).into_iter().collect(),
        message: "something".to_owned(),
        line: 3,
        column: 1,
    }
}

#[test]
fn every_compiler_finding_prints_its_identifier_unaltered() {
    for _ in 0..2 {
        let text = diagnostic_text(&diagnostic(
            Some("clippy::len_zero"),
            "error",
            Some("src/main.rs"),
        ));
        assert!(
            text.ends_with("  = doc: rust-qa rule-doc clippy::len_zero\n"),
            "{text}"
        );
    }
    assert!(!diagnostic_text(&diagnostic(None, "error", None)).contains("= doc"));
}

#[test]
fn a_dylint_finding_with_no_lint_name_is_its_own_failure() {
    let lints = vec!["my_lint".to_owned()];
    let source = Some("src/lib.rs");
    assert_eq!(
        dylint_disposition(&diagnostic(Some("my_lint"), "warning", source), &lints),
        Disposition::Lint("my_lint".to_owned())
    );
    assert_eq!(
        dylint_disposition(&diagnostic(None, "warning", source), &lints),
        Disposition::Nameless
    );
    // The count cargo prints at the end ("1 warning emitted") has no span.
    assert_eq!(
        dylint_disposition(&diagnostic(None, "warning", None), &lints),
        Disposition::Other
    );
    assert_eq!(
        dylint_disposition(&diagnostic(Some("dead_code"), "warning", source), &lints),
        Disposition::Other
    );
    assert_eq!(
        dylint_disposition(&diagnostic(Some("E0308"), "error", source), &lints),
        Disposition::Error
    );
}

#[test]
fn cargo_deny_runs_with_json_and_each_code_prints_as_an_identifier_that_resolves() {
    let ctx = context(Config::default());
    assert_eq!(
        line(Tool::Deny, &ctx),
        vec!["cargo deny --format json --workspace --all-features check"]
    );
    let stream = concat!(
        r#"{"fields":{"code":"unlicensed","graphs":[{"Krate":{"name":"a","version":"0.1.0"}}],"message":"a = 0.1.0 is unlicensed","severity":"error"},"type":"diagnostic"}"#,
        "\n",
        r#"{"fields":{"licenses":{"errors":1,"helps":0,"notes":0,"warnings":0}},"type":"summary"}"#,
        "\nnot json\n"
    );
    let report = deny_report(stream).join("\n");
    assert!(
        report.contains("error[cargo-deny::unlicensed]: a = 0.1.0 is unlicensed"),
        "{report}"
    );
    assert!(report.contains("= crate: a 0.1.0"), "{report}");
    assert!(
        report.contains("= doc: rust-qa rule-doc cargo-deny::unlicensed"),
        "{report}"
    );
    assert!(report.contains("not json"), "{report}");
}
