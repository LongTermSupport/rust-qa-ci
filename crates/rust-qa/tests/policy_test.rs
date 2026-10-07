//! Record policy: budget caps, breadth, review deadlines, trust tiers, co-signing and the ratchet.
//!
//! Glob paths are assembled with [`deep`] and [`STAR`] so no string literal here opens a comment.
#![cfg(test)]

use std::fmt::Write as _;
use std::path::Path;

use rust_qa::builtin::{RECORD_BUDGET, RECORD_EXPIRED, RECORD_INVALID, RECORD_RATCHET};
use rust_qa::date::Date;
use rust_qa::finding::Finding;
use rust_qa::policy::{
    Cosign, Input, RecordPolicy, Status, Tier, gate, is_broad, looks_automated, previous_record,
    remove_exceptions, review, set_high_water,
};
use rust_qa::record::{Exception, Ratchet, Record};

const GOOD: &str = "The generated parser module is regenerated from the grammar on every build.";
const DECIDER: &str = "alice";
const STAR: &str = "*";

fn deep(dir: &str) -> String {
    format!("{dir}/{STAR}{STAR}")
}

fn today() -> Date {
    Date::parse("2026-10-07").unwrap()
}

fn policy() -> RecordPolicy {
    let mut policy = RecordPolicy {
        max_total: 6,
        max_per_rule: 3,
        max_policy: 1,
        max_review_days: 180,
        deciders: vec![DECIDER.to_owned()],
        ..RecordPolicy::default()
    };
    policy.trust.models.insert("big".to_owned(), Tier::High);
    policy.trust.models.insert("mid".to_owned(), Tier::Medium);
    policy.trust.models.insert("small".to_owned(), Tier::Low);
    policy.trust.high.max_review_days = Some(90);
    policy.trust.medium.max_review_days = Some(30);
    policy.trust.medium.max_count = Some(3);
    policy.trust.low.max_review_days = Some(14);
    policy.trust.low.max_count = Some(1);
    policy
}

/// A valid entry decided by a human today, due for review in 60 days.
fn entry(rule: &str, path: &str) -> Exception {
    Exception {
        rule: rule.to_owned(),
        path: path.to_owned(),
        justification: GOOD.to_owned(),
        decided_by: Some(DECIDER.to_owned()),
        added_by_model: Some("human".to_owned()),
        decided_on: Some(today()),
        review_by: Some(today().add_days(60)),
        policy: false,
    }
}

fn ratchet(high_water: usize) -> Ratchet {
    Ratchet {
        high_water,
        decided_by: DECIDER.to_owned(),
        decided_on: today(),
        reason: "Initial ceiling: the record held these entries when the budget was adopted."
            .to_owned(),
    }
}

fn signed(exceptions: Vec<Exception>) -> Record {
    let high_water = exceptions.len();
    Record {
        ratchet: Some(ratchet(high_water)),
        exceptions,
    }
}

fn check(record: &Record, policy: &RecordPolicy, previous: Option<&Record>) -> Vec<Finding> {
    review(&Input {
        record,
        record_path: Path::new("qa/record.toml"),
        policy,
        today: today(),
        previous,
    })
    .findings
}

fn rules(findings: &[Finding]) -> Vec<&'static str> {
    findings.iter().map(|f| f.rule).collect()
}

fn assert_one(findings: &[Finding], rule: &str, fragment: &str) {
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].rule, rule, "{findings:?}");
    assert!(
        findings[0].message.contains(fragment),
        "`{}` lacks `{fragment}`",
        findings[0].message
    );
}

#[test]
fn a_small_signed_current_record_passes() {
    let record = signed(vec![entry("dead_code", "src/a.rs")]);
    assert_eq!(check(&record, &policy(), None), vec![]);
}

#[test]
fn an_empty_record_needs_no_ratchet() {
    assert_eq!(check(&Record::default(), &policy(), None), vec![]);
}

// Budget

#[test]
fn total_beyond_the_budget_fails() {
    let mut policy = policy();
    policy.max_per_rule = 6;
    let record = signed(
        (0..7)
            .map(|i| entry("dead_code", &format!("src/{i}.rs")))
            .collect(),
    );
    let findings = check(&record, &policy, None);
    assert!(
        findings
            .iter()
            .any(|f| f.rule == RECORD_BUDGET && f.message.contains("7 exceptions exceed")),
        "{findings:?}"
    );
}

#[test]
fn one_rule_beyond_its_per_rule_cap_fails_and_names_the_rule() {
    let record = signed(
        (0..4)
            .map(|i| entry("clippy::unwrap_used", &format!("src/{i}.rs")))
            .collect(),
    );
    assert_one(
        &check(&record, &policy(), None),
        RECORD_BUDGET,
        "clippy::unwrap_used",
    );
}

#[test]
fn per_rule_counting_normalises_hyphenated_names() {
    let record = signed(vec![
        entry("clippy::unwrap_used", "src/a.rs"),
        entry("clippy::unwrap-used", "src/b.rs"),
        entry("clippy::unwrap_used", "src/c.rs"),
        entry("clippy::unwrap-used", "src/d.rs"),
    ]);
    assert_eq!(rules(&check(&record, &policy(), None)), vec![RECORD_BUDGET]);
}

// Breadth

#[test]
fn glob_paths_are_broad_and_literal_files_are_not() {
    for path in [
        "**".to_owned(),
        deep("src"),
        format!("src/{STAR}.rs"),
        format!("crates/{STAR}/src/lib.rs"),
        "src/[ab].rs".to_owned(),
        "src/{a,b}.rs".to_owned(),
        "a?.rs".to_owned(),
    ] {
        assert!(is_broad(&path), "{path} should be broad");
    }
    for path in ["src/lib.rs", "clippy.toml", "crates/x/Cargo.toml"] {
        assert!(!is_broad(path), "{path} should be narrow");
    }
}

#[test]
fn a_broad_path_without_the_policy_marker_is_rejected() {
    let record = signed(vec![entry("dead_code", &deep("src"))]);
    assert_one(
        &check(&record, &policy(), None),
        RECORD_INVALID,
        "policy = true",
    );
}

#[test]
fn a_policy_level_exception_passes_within_its_own_cap() {
    let mut broad = entry("dead_code", &deep("src"));
    broad.policy = true;
    assert_eq!(check(&signed(vec![broad]), &policy(), None), vec![]);
}

#[test]
fn policy_level_exceptions_beyond_their_stricter_cap_fail() {
    let mut a = entry("dead_code", &deep("src"));
    a.policy = true;
    let mut b = entry("unused", &deep("tests"));
    b.policy = true;
    assert_one(
        &check(&signed(vec![a, b]), &policy(), None),
        RECORD_BUDGET,
        "policy-level",
    );
}

#[test]
fn the_policy_marker_on_a_single_file_is_rejected() {
    let mut narrow = entry("dead_code", "src/a.rs");
    narrow.policy = true;
    assert_one(
        &check(&signed(vec![narrow]), &policy(), None),
        RECORD_INVALID,
        "single file",
    );
}

// Required fields and review deadlines

#[test]
fn missing_provenance_fields_are_each_reported() {
    let mut bare = entry("dead_code", "src/a.rs");
    bare.decided_by = None;
    bare.added_by_model = None;
    bare.decided_on = None;
    bare.review_by = None;
    let findings = check(&signed(vec![bare]), &policy(), None);
    assert_eq!(rules(&findings), vec![RECORD_INVALID]);
    for field in ["decided_by", "added_by_model", "decided_on", "review_by"] {
        assert!(findings[0].message.contains(field), "{findings:?}");
    }
}

#[test]
fn an_expired_review_date_fails_until_re_decided() {
    let mut old = entry("dead_code", "src/a.rs");
    old.decided_on = Some(today().add_days(-100));
    old.review_by = Some(today().add_days(-1));
    assert_one(
        &check(&signed(vec![old.clone()]), &policy(), None),
        RECORD_EXPIRED,
        "re-decide",
    );
    old.review_by = Some(today());
    assert_eq!(
        check(&signed(vec![old]), &policy(), None),
        vec![],
        "due today still passes"
    );
}

#[test]
fn a_review_date_beyond_the_horizon_is_rejected() {
    let mut far = entry("dead_code", "src/a.rs");
    far.review_by = Some(today().add_days(181));
    assert_one(
        &check(&signed(vec![far]), &policy(), None),
        RECORD_INVALID,
        "180 days",
    );
}

#[test]
fn a_decision_dated_in_the_future_is_rejected() {
    let mut future = entry("dead_code", "src/a.rs");
    future.decided_on = Some(today().add_days(1));
    future.review_by = Some(today().add_days(10));
    assert_one(
        &check(&signed(vec![future]), &policy(), None),
        RECORD_INVALID,
        "future",
    );
}

#[test]
fn a_review_date_before_the_decision_is_rejected() {
    let mut odd = entry("dead_code", "src/a.rs");
    odd.decided_on = Some(today().add_days(-10));
    odd.review_by = Some(today().add_days(-20));
    let findings = check(&signed(vec![odd]), &policy(), None);
    assert!(
        findings
            .iter()
            .any(|f| f.rule == RECORD_INVALID && f.message.contains("before")),
        "{findings:?}"
    );
}

// Trust tiers

#[test]
fn models_map_to_tiers_by_longest_prefix_and_human_is_reserved() {
    let mut policy = policy();
    policy.trust.models.insert("big-lite".to_owned(), Tier::Low);
    assert_eq!(policy.tier("human"), Tier::Human);
    assert_eq!(policy.tier("Human"), Tier::Human);
    assert_eq!(policy.tier("big-5-5"), Tier::High);
    assert_eq!(policy.tier("big-lite-2"), Tier::Low);
    assert_eq!(policy.tier("mid-4"), Tier::Medium);
    assert_eq!(policy.tier("other-model"), Tier::Unknown);
    assert!(Tier::Human > Tier::High && Tier::High > Tier::Medium);
    assert!(Tier::Medium > Tier::Low && Tier::Low > Tier::Unknown);
}

#[test]
fn an_unknown_model_fails_the_gate() {
    let mut unknown = entry("dead_code", "src/a.rs");
    unknown.added_by_model = Some("mystery-1".to_owned());
    assert_one(
        &check(&signed(vec![unknown]), &policy(), None),
        RECORD_INVALID,
        "mystery-1",
    );
}

#[test]
fn a_human_entry_must_name_an_allowlisted_decider() {
    let mut claimed = entry("dead_code", "src/a.rs");
    claimed.decided_by = Some("mallory".to_owned());
    assert_one(
        &check(&signed(vec![claimed]), &policy(), None),
        RECORD_INVALID,
        "deciders",
    );
}

#[test]
fn lower_tiers_get_shorter_review_horizons() {
    let mut mid = entry("dead_code", "src/a.rs");
    mid.added_by_model = Some("mid-4".to_owned());
    mid.review_by = Some(today().add_days(30));
    assert_eq!(check(&signed(vec![mid.clone()]), &policy(), None), vec![]);
    mid.review_by = Some(today().add_days(31));
    assert_one(
        &check(&signed(vec![mid]), &policy(), None),
        RECORD_INVALID,
        "30 days",
    );
    let mut small = entry("dead_code", "src/a.rs");
    small.added_by_model = Some("small-1".to_owned());
    small.review_by = Some(today().add_days(15));
    assert_one(
        &check(&signed(vec![small]), &policy(), None),
        RECORD_INVALID,
        "14 days",
    );
}

#[test]
fn lower_tiers_have_lower_count_caps() {
    let smalls: Vec<Exception> = (0..2)
        .map(|i| {
            let mut e = entry("dead_code", &format!("src/{i}.rs"));
            e.added_by_model = Some("small".to_owned());
            e.review_by = Some(today().add_days(7));
            e
        })
        .collect();
    assert_one(
        &check(&signed(smalls), &policy(), None),
        RECORD_BUDGET,
        "low",
    );
}

#[test]
fn security_rules_from_low_tiers_need_a_human_co_signature() {
    let mut policy = policy();
    policy.trust.cosign.push(Cosign {
        rules: vec!["sensitive_*".to_owned(), "cargo-deny::*".to_owned()],
        min_tier: Tier::High,
    });
    let mut agent = entry("cargo-deny::RUSTSEC-2020-0001", "deny.toml");
    agent.added_by_model = Some("mid-4".to_owned());
    agent.decided_by = Some("mid-4".to_owned());
    agent.review_by = Some(today().add_days(10));
    assert_one(
        &check(&signed(vec![agent.clone()]), &policy, None),
        RECORD_INVALID,
        "co-sign",
    );
    agent.decided_by = Some(DECIDER.to_owned());
    assert_eq!(check(&signed(vec![agent.clone()]), &policy, None), vec![]);
    let mut high = agent;
    high.added_by_model = Some("big-2".to_owned());
    high.decided_by = Some("big-2".to_owned());
    assert_eq!(
        check(&signed(vec![high]), &policy, None),
        vec![],
        "the high tier meets the threshold"
    );
}

#[test]
fn rules_outside_cosign_categories_may_be_decided_by_an_agent() {
    let mut agent = entry("dead_code", "src/a.rs");
    agent.added_by_model = Some("big-2".to_owned());
    agent.decided_by = Some("big-2".to_owned());
    assert_eq!(check(&signed(vec![agent]), &policy(), None), vec![]);
}

// Ratchet

#[test]
fn a_non_empty_record_without_a_ratchet_fails() {
    let record = Record {
        ratchet: None,
        exceptions: vec![entry("dead_code", "src/a.rs")],
    };
    assert_one(
        &check(&record, &policy(), None),
        RECORD_RATCHET,
        "[ratchet]",
    );
}

#[test]
fn growth_beyond_the_high_water_fails() {
    let mut record = signed(vec![
        entry("dead_code", "src/a.rs"),
        entry("unused", "src/b.rs"),
    ]);
    record.ratchet = Some(ratchet(1));
    assert_one(&check(&record, &policy(), None), RECORD_RATCHET, "grew");
}

#[test]
fn shrinking_below_the_high_water_asks_to_lower_it() {
    let mut record = signed(vec![entry("dead_code", "src/a.rs")]);
    record.ratchet = Some(ratchet(3));
    let verdict = review(&Input {
        record: &record,
        record_path: Path::new("qa/record.toml"),
        policy: &policy(),
        today: today(),
        previous: None,
    });
    assert_eq!(verdict.lower_to, Some(1));
    assert_eq!(verdict.findings, vec![], "lowering is reported separately");
}

#[test]
fn a_high_water_above_the_budget_fails() {
    let mut record = signed(vec![entry("dead_code", "src/a.rs")]);
    record.ratchet = Some(ratchet(7));
    let findings = check(&record, &policy(), None);
    assert!(
        findings
            .iter()
            .any(|f| f.rule == RECORD_BUDGET && f.message.contains("high_water")),
        "{findings:?}"
    );
}

#[test]
fn the_ratchet_must_be_signed_by_an_allowlisted_human_with_a_reason() {
    let mut record = signed(vec![entry("dead_code", "src/a.rs")]);
    if let Some(r) = record.ratchet.as_mut() {
        r.decided_by = "ci-bot".to_owned();
        r.reason = "needed".to_owned();
    }
    let findings = check(&record, &policy(), None);
    assert_eq!(rules(&findings), vec![RECORD_RATCHET]);
    assert!(
        findings[0].message.contains("ci-bot") && findings[0].message.contains("reason"),
        "{findings:?}"
    );
}

#[test]
fn a_raise_against_history_needs_a_fresh_signature() {
    let before = signed(vec![entry("dead_code", "src/a.rs")]);
    let mut after = signed(vec![
        entry("dead_code", "src/a.rs"),
        entry("unused", "src/b.rs"),
    ]);
    assert_one(
        &check(&after, &policy(), Some(&before)),
        RECORD_RATCHET,
        "raised from 1 to 2",
    );
    let mut old_before = before.clone();
    if let Some(r) = old_before.ratchet.as_mut() {
        r.decided_on = today().add_days(-60);
    }
    if let Some(r) = after.ratchet.as_mut() {
        r.decided_on = today().add_days(-30);
    }
    assert_one(
        &check(&after, &policy(), Some(&old_before)),
        RECORD_RATCHET,
        "14 days",
    );
    if let Some(r) = after.ratchet.as_mut() {
        r.decided_on = today();
    }
    assert_eq!(check(&after, &policy(), Some(&old_before)), vec![]);
}

fn git(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
        .status;
    assert!(status.success(), "git {args:?}");
}

fn record_text(high_water: usize, decided_on: Date, entries: &[&str]) -> String {
    let mut text = format!(
        "[ratchet]\nhigh_water = {high_water}\ndecided_by = \"{DECIDER}\"\ndecided_on = {decided_on}\nreason = \"{GOOD}\"\n"
    );
    for path in entries {
        let _ = write!(
            text,
            "\n[[exception]]\nrule = \"dead_code\"\npath = \"{path}\"\njustification = \"{GOOD}\"\n\
             decided_by = \"{DECIDER}\"\nadded_by_model = \"human\"\ndecided_on = {decided_on}\n\
             review_by = {}\n",
            decided_on.add_days(30)
        );
    }
    text
}

#[test]
fn the_gate_compares_the_ceiling_with_the_committed_record() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let today = Date::today();
    std::fs::create_dir_all(root.join("qa")).unwrap();
    let file = root.join("qa/record.toml");
    std::fs::write(&file, record_text(1, today, &["src/a.rs"])).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "record"]);
    assert!(previous_record(root, Path::new("qa/record.toml"), "no-such-ref").is_some());
    std::fs::write(&file, record_text(2, today, &["src/a.rs", "src/b.rs"])).unwrap();
    let report = gate(root, &policy(), today, false).unwrap();
    assert_eq!(rules(&report.findings), vec![RECORD_RATCHET], "{report:?}");
    assert!(report.findings[0].message.contains("raised from 1 to 2"));
    assert!(report.notes.is_empty(), "{:?}", report.notes);
}

#[test]
fn the_gate_notes_when_history_is_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("qa")).unwrap();
    std::fs::write(
        root.join("qa/record.toml"),
        record_text(1, today(), &["src/a.rs"]),
    )
    .unwrap();
    let report = gate(root, &policy(), today(), false).unwrap();
    assert_eq!(report.findings, vec![]);
    assert!(report.notes[0].contains("no committed history"));
    assert!(report.status.to_string().contains("1/6 exceptions"));
}

#[test]
fn adopting_the_ratchet_at_the_existing_count_is_not_a_raise() {
    let legacy = Record {
        ratchet: None,
        exceptions: vec![entry("dead_code", "src/a.rs")],
    };
    let adopted = signed(vec![entry("dead_code", "src/a.rs")]);
    assert_eq!(check(&adopted, &policy(), Some(&legacy)), vec![]);
}

// Configuration

#[test]
fn automation_identities_are_recognised() {
    for id in [
        "ci-bot",
        "dependabot[bot]",
        "github-actions",
        "release_agent",
        "Claude",
        "gpt",
        "renovate",
    ] {
        assert!(looks_automated(id), "{id} should look automated");
    }
    for id in ["alice", "joseph", "maintainer", "Lucia"] {
        assert!(!looks_automated(id), "{id} should look human");
    }
}

type Mutation = Box<dyn Fn(&mut RecordPolicy)>;

#[test]
fn policy_validation_rejects_unsafe_settings() {
    assert!(policy().validate().is_ok());
    let cases: Vec<(&str, Mutation)> = vec![
        (
            "automation",
            Box::new(|p| p.deciders.push("deploy-bot".to_owned())),
        ),
        ("empty", Box::new(|p| p.deciders.push(" ".to_owned()))),
        ("max_per_rule", Box::new(|p| p.max_per_rule = 7)),
        ("max_policy", Box::new(|p| p.max_policy = 4)),
        (
            "human",
            Box::new(|p| {
                p.trust.models.insert("x".to_owned(), Tier::Human);
            }),
        ),
        (
            "stricter",
            Box::new(|p| p.trust.low.max_review_days = Some(60)),
        ),
        ("stricter", Box::new(|p| p.trust.low.max_count = Some(5))),
        (
            "glob",
            Box::new(|p| {
                p.trust.cosign.push(Cosign {
                    rules: vec!["[".to_owned()],
                    min_tier: Tier::High,
                });
            }),
        ),
    ];
    for (fragment, mutate) in cases {
        let mut bad = policy();
        mutate(&mut bad);
        let error = bad.validate().expect_err(fragment);
        assert!(error.contains(fragment), "`{error}` lacks `{fragment}`");
    }
}

// Status

#[test]
fn status_reports_counts_against_budgets_tiers_and_deadlines() {
    let mut mid = entry("unused", "src/b.rs");
    mid.added_by_model = Some("mid-1".to_owned());
    mid.decided_on = Some(today().add_days(-40));
    mid.review_by = Some(today().add_days(-10));
    let record = signed(vec![entry("dead_code", "src/a.rs"), mid]);
    let text = Status::of(&record, &policy()).to_string();
    for fragment in [
        "2/6 exceptions",
        "high water 2",
        "per rule: max 1/3",
        "policy-level: 0/1",
        "human 1",
        "medium 1/3",
        "soonest review: 2026-09-27 `unused` at `src/b.rs`",
        "oldest decision: 2026-08-28 `unused` at `src/b.rs`",
    ] {
        assert!(
            text.contains(fragment),
            "status lacks `{fragment}`:\n{text}"
        );
    }
}

// Text edits

const FILE: &str = "# header comment\n\n[ratchet]\nhigh_water = 3  # ceiling\ndecided_by = \"alice\"\n\n\
# first\n[[exception]]\nrule = \"a\"\n\n# second\n[[exception]]\nrule = \"b\"\n\n[[exception]]\nrule = \"c\"\n";

#[test]
fn set_high_water_rewrites_only_the_ratchet_value() {
    let out = set_high_water(FILE, 2).unwrap();
    assert!(out.contains("high_water = 2  # ceiling\n"), "{out}");
    assert_eq!(out.replace("high_water = 2", "high_water = 3"), FILE);
    assert!(set_high_water("[[exception]]\nrule = \"a\"\n", 1).is_none());
}

#[test]
fn remove_exceptions_drops_whole_blocks_with_their_leading_comments() {
    let out = remove_exceptions(FILE, &[1]);
    assert!(!out.contains("rule = \"b\""), "{out}");
    assert!(!out.contains("# second"), "{out}");
    assert!(
        out.contains("# first\n[[exception]]\nrule = \"a\"\n"),
        "{out}"
    );
    assert!(out.contains("rule = \"c\""), "{out}");
    assert!(out.contains("[ratchet]"), "{out}");
    let parsed: toml::Value = toml::from_str(&remove_exceptions(FILE, &[0, 2])).unwrap();
    assert_eq!(parsed["exception"].as_array().unwrap().len(), 1);
}
