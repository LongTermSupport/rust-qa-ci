//! Anti-bloat controls for the project record.
//!
//! Exception records grow by default: each entry is cheap to add and nobody is responsible for
//! removing it. These checks make growth expensive and shrinkage automatic:
//!
//! - budgets (`rqaci.record.budget`): caps on the total, per rule id, on broad policy-level entries
//!   and per trust tier;
//! - breadth and provenance (`rqaci.record.invalid`): one file per entry unless marked
//!   `policy = true`; every entry names who decided it, which model (or `human`) wrote it, when it
//!   was decided and when it must be reviewed, within a horizon that shrinks for lower trust tiers;
//!   rule categories can require a human co-signature below a tier;
//! - expiry (`rqaci.record.expired`): an entry past `review_by` fails until re-decided or removed;
//! - the ratchet (`rqaci.record.ratchet`): a signed `high_water` ceiling that the gate lowers
//!   automatically and that only a configured human decider may raise, with a fresh signature.
//!
//! Trade-offs are documented in the README, "Project record".

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::Error;
use crate::builtin::{RECORD_BUDGET, RECORD_EXPIRED, RECORD_INVALID, RECORD_RATCHET};
use crate::date::Date;
use crate::finding::Finding;
use crate::record::{Exception, Record, justification_problem, normalise_rule};

/// A raise of `high_water` must be signed within this many days.
pub const RAISE_WINDOW_DAYS: i64 = 14;

/// Words that mark an identifier as an automation identity rather than a person.
const AUTOMATION_WORDS: [&str; 24] = [
    "bot",
    "bots",
    "agent",
    "agents",
    "ai",
    "llm",
    "model",
    "assistant",
    "automation",
    "automated",
    "ci",
    "actions",
    "dependabot",
    "renovate",
    "claude",
    "gpt",
    "chatgpt",
    "copilot",
    "codex",
    "gemini",
    "opus",
    "sonnet",
    "haiku",
    "llama",
];

/// How far an entry's author is trusted, lowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// A model id that matches no configured prefix; always rejected.
    Unknown,
    /// Least-trusted model class.
    Low,
    /// Middle model class.
    Medium,
    /// Most-trusted model class.
    High,
    /// Written by a person (`added_by_model = "human"`).
    Human,
}

impl Tier {
    /// Tiers that can hold entries, most trusted first.
    pub const RANKED: [Self; 4] = [Self::Human, Self::High, Self::Medium, Self::Low];

    /// Lower-case name, as written in configuration.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Human => "human",
        }
    }
}

/// Record policy from the `[record]` table of the configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RecordPolicy {
    /// Project record file, relative to the root.
    pub path: PathBuf,
    /// Hard cap on the number of exceptions.
    pub max_total: usize,
    /// Cap on exceptions for any one rule id.
    pub max_per_rule: usize,
    /// Cap on broad (glob) exceptions marked `policy = true`.
    pub max_policy: usize,
    /// Longest allowed gap between `decided_on` and `review_by`, in days, for any tier.
    pub max_review_days: u32,
    /// People who may sign the ratchet, mark entries `human` and co-sign restricted rules.
    pub deciders: Vec<String>,
    /// Git ref whose merge base supplies the previous record for raise detection.
    pub history_base: String,
    /// Trust tiers by authoring model.
    pub trust: Trust,
}

impl Default for RecordPolicy {
    fn default() -> Self {
        Self {
            path: PathBuf::from("qa/record.toml"),
            max_total: 10,
            max_per_rule: 3,
            max_policy: 0,
            max_review_days: 180,
            deciders: Vec::new(),
            history_base: "origin/main".to_owned(),
            trust: Trust::default(),
        }
    }
}

/// Trust tiers: which models map to which tier, per-tier limits and co-sign rules.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Trust {
    /// Model id prefix (case-insensitive, longest wins) to tier; `human` is reserved.
    pub models: BTreeMap<String, Tier>,
    /// Limits for entries written by people.
    pub human: TierLimits,
    /// Limits for the high tier.
    pub high: TierLimits,
    /// Limits for the medium tier.
    pub medium: TierLimits,
    /// Limits for the low tier.
    pub low: TierLimits,
    /// Rule categories that need a human decider below a tier.
    pub cosign: Vec<Cosign>,
}

impl Default for Trust {
    fn default() -> Self {
        Self {
            models: BTreeMap::new(),
            human: TierLimits::default(),
            high: TierLimits {
                max_review_days: Some(90),
                max_count: None,
            },
            medium: TierLimits {
                max_review_days: Some(30),
                max_count: Some(3),
            },
            low: TierLimits {
                max_review_days: Some(14),
                max_count: Some(1),
            },
            cosign: Vec::new(),
        }
    }
}

/// Per-tier limits; unset values fall back to the record-wide caps.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TierLimits {
    /// Review horizon in days (never longer than `max_review_days`).
    pub max_review_days: Option<u32>,
    /// Entries this tier may hold (never more than `max_total`).
    pub max_count: Option<usize>,
}

/// A rule category whose exceptions need a human decider when written below `min_tier`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cosign {
    /// Rule id globs (for example `cargo-deny::*`).
    pub rules: Vec<String>,
    /// Lowest tier that may decide alone.
    pub min_tier: Tier,
}

impl RecordPolicy {
    /// Tier for an `added_by_model` value.
    #[must_use]
    pub fn tier(&self, model: &str) -> Tier {
        let model = model.trim().to_lowercase();
        if model == "human" {
            return Tier::Human;
        }
        self.trust
            .models
            .iter()
            .filter(|(prefix, _)| {
                !prefix.trim().is_empty() && model.starts_with(&prefix.trim().to_lowercase())
            })
            .max_by_key(|(prefix, _)| prefix.trim().len())
            .map_or(Tier::Unknown, |(_, tier)| *tier)
    }

    fn limits(&self, tier: Tier) -> TierLimits {
        match tier {
            Tier::Human => self.trust.human,
            Tier::High => self.trust.high,
            Tier::Medium => self.trust.medium,
            Tier::Low | Tier::Unknown => self.trust.low,
        }
    }

    /// Effective review horizon for a tier.
    #[must_use]
    pub fn review_days(&self, tier: Tier) -> u32 {
        self.limits(tier)
            .max_review_days
            .map_or(self.max_review_days, |days| days.min(self.max_review_days))
    }

    /// Effective entry cap for a tier (`None`: only `max_total` applies).
    #[must_use]
    pub fn count_cap(&self, tier: Tier) -> Option<usize> {
        self.limits(tier)
            .max_count
            .map(|count| count.min(self.max_total))
    }

    /// Whether `who` is a configured decider.
    #[must_use]
    pub fn is_decider(&self, who: &str) -> bool {
        let who = who.trim();
        !who.is_empty() && self.deciders.iter().any(|decider| decider.trim() == who)
    }

    /// Highest co-sign threshold among categories matching `rule`.
    fn cosign_threshold(&self, rule: &str) -> Option<Tier> {
        let rule = normalise_rule(rule);
        self.trust
            .cosign
            .iter()
            .filter(|category| {
                category.rules.iter().any(|glob| {
                    globset::Glob::new(glob)
                        .is_ok_and(|glob| glob.compile_matcher().is_match(&rule))
                })
            })
            .map(|category| category.min_tier)
            .max()
    }

    /// Checks the policy itself: human deciders, coherent caps, and limits that get stricter for
    /// lower tiers.
    ///
    /// # Errors
    /// Returns every problem found, joined into one message.
    pub fn validate(&self) -> Result<(), String> {
        let mut problems = Vec::new();
        for decider in &self.deciders {
            if decider.trim().is_empty() {
                problems.push("deciders: empty identifier".to_owned());
            } else if looks_automated(decider) {
                problems.push(format!(
                    "deciders: `{decider}` looks like an automation identity; deciders must be people"
                ));
            }
        }
        if self.max_per_rule > self.max_total {
            problems.push(format!(
                "max_per_rule ({}) exceeds max_total ({})",
                self.max_per_rule, self.max_total
            ));
        }
        if self.max_policy > self.max_per_rule {
            problems.push(format!(
                "max_policy ({}) must not exceed max_per_rule ({}); policy-level exceptions have the stricter cap",
                self.max_policy, self.max_per_rule
            ));
        }
        for (prefix, tier) in &self.trust.models {
            if prefix.trim().is_empty() {
                problems.push("trust.models: empty model prefix".to_owned());
            }
            if matches!(tier, Tier::Human | Tier::Unknown) {
                problems.push(format!(
                    "trust.models.{prefix} maps to `{}`; models map to high, medium or low (`human` is reserved for people)",
                    tier.name()
                ));
            }
        }
        for pair in Tier::RANKED.windows(2) {
            let [higher, lower] = pair else { continue };
            let (higher, lower) = (*higher, *lower);
            if self.review_days(lower) > self.review_days(higher) {
                problems.push(format!(
                    "trust.{}.max_review_days ({}) is longer than trust.{} ({}); lower tiers must be stricter",
                    lower.name(),
                    self.review_days(lower),
                    higher.name(),
                    self.review_days(higher)
                ));
            }
            let cap = |tier| self.count_cap(tier).unwrap_or(self.max_total);
            if cap(lower) > cap(higher) {
                problems.push(format!(
                    "trust.{}.max_count ({}) is larger than trust.{} ({}); lower tiers must be stricter",
                    lower.name(),
                    cap(lower),
                    higher.name(),
                    cap(higher)
                ));
            }
        }
        for category in &self.trust.cosign {
            for glob in &category.rules {
                if globset::Glob::new(glob).is_err() {
                    problems.push(format!("trust.cosign: invalid rule glob `{glob}`"));
                }
            }
            if category.min_tier == Tier::Unknown {
                problems.push("trust.cosign: min_tier cannot be `unknown`".to_owned());
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("; "))
        }
    }
}

/// Whether an identifier names an automation identity (a bot, CI job, agent or model).
#[must_use]
pub fn looks_automated(id: &str) -> bool {
    id.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| AUTOMATION_WORDS.contains(&word))
}

/// Whether an exception path is a glob (covers more than one file).
#[must_use]
pub fn is_broad(path: &str) -> bool {
    path.contains(['*', '?', '[', '{'])
}

/// The ceiling a record commits to: its ratchet, or its size when it has none.
#[must_use]
pub fn ceiling(record: &Record) -> usize {
    record
        .ratchet
        .as_ref()
        .map_or(record.exceptions.len(), |ratchet| ratchet.high_water)
}

/// What a review looks at.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    /// The record as it is now.
    pub record: &'a Record,
    /// Record path, for findings.
    pub record_path: &'a Path,
    /// Policy from configuration.
    pub policy: &'a RecordPolicy,
    /// Today's date (UTC).
    pub today: Date,
    /// The record before this change, when history is available.
    pub previous: Option<&'a Record>,
}

/// Outcome of a review.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Verdict {
    /// Policy violations.
    pub findings: Vec<Finding>,
    /// The record shrank: `high_water` should be lowered to this count.
    pub lower_to: Option<usize>,
}

/// Applies every policy control to the record.
#[must_use]
pub fn review(input: &Input<'_>) -> Verdict {
    let mut findings = Vec::new();
    for (index, exception) in input.record.exceptions.iter().enumerate() {
        entry(input, index, exception, &mut findings);
    }
    budget(input, &mut findings);
    let lower_to = ratchet(input, &mut findings);
    Verdict { findings, lower_to }
}

fn finding(rule: &'static str, input: &Input<'_>, message: String) -> Finding {
    Finding {
        rule,
        path: input.record_path.to_path_buf(),
        line: 0,
        message,
    }
}

fn present(value: Option<&String>) -> Option<&str> {
    value
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
}

fn entry(input: &Input<'_>, index: usize, exception: &Exception, out: &mut Vec<Finding>) {
    let policy = input.policy;
    let label = format!(
        "exception #{} (rule `{}`, path `{}`)",
        index + 1,
        exception.rule,
        exception.path
    );
    let decided_by = present(exception.decided_by.as_ref());
    let model = present(exception.added_by_model.as_ref());
    let mut problems = Vec::new();
    let missing: Vec<&str> = [
        ("decided_by", decided_by.is_none()),
        ("added_by_model", model.is_none()),
        ("decided_on", exception.decided_on.is_none()),
        ("review_by", exception.review_by.is_none()),
    ]
    .into_iter()
    .filter_map(|(field, absent)| absent.then_some(field))
    .collect();
    if !missing.is_empty() {
        problems.push(format!("is missing {}", missing.join(", ")));
    }
    let tier = model.map(|model| policy.tier(model));
    let co_signed = decided_by.is_some_and(|who| policy.is_decider(who));
    match (tier, model) {
        (Some(Tier::Unknown), Some(model)) => problems.push(format!(
            "added_by_model `{model}` maps to no trust tier; list its prefix under [record.trust] models, or write \"human\""
        )),
        (Some(Tier::Human), _) if decided_by.is_some() && !co_signed => problems.push(format!(
            "is marked human but decided_by `{}` is not one of the configured deciders",
            decided_by.unwrap_or_default()
        )),
        (Some(tier), _) if tier != Tier::Unknown => {
            if let Some(threshold) = policy.cosign_threshold(&exception.rule)
                && tier < threshold
                && !co_signed
            {
                problems.push(format!(
                    "needs a human co-sign: written by a {}-tier model below the `{}` threshold for this rule, so decided_by must be one of the configured deciders",
                    tier.name(),
                    threshold.name()
                ));
            }
        }
        _ => {}
    }
    if let Some(on) = exception.decided_on
        && on > input.today
    {
        problems.push(format!("decided_on {on} is in the future"));
    }
    if let (Some(on), Some(by)) = (exception.decided_on, exception.review_by) {
        let horizon = policy.review_days(tier.unwrap_or(Tier::Low));
        if by < on {
            problems.push(format!("review_by {by} is before decided_on {on}"));
        } else if on.days_until(by) > i64::from(horizon) {
            problems.push(format!(
                "review_by {by} is more than {horizon} days after decided_on {on} (the limit for the {} tier)",
                tier.unwrap_or(Tier::Low).name()
            ));
        }
    }
    let broad = is_broad(&exception.path);
    if broad && !exception.policy {
        problems.push(format!(
            "path `{}` is a glob; narrow it to one file, or mark a deliberate policy-level exception with `policy = true` (separately capped)",
            exception.path
        ));
    }
    if !broad && exception.policy {
        problems
            .push("is marked policy = true but names a single file; drop the marker".to_owned());
    }
    if !problems.is_empty() {
        out.push(finding(
            RECORD_INVALID,
            input,
            format!("{label} {}", problems.join("; ")),
        ));
    }
    if let Some(by) = exception.review_by
        && by < input.today
    {
        out.push(finding(
            RECORD_EXPIRED,
            input,
            format!(
                "{label} was due for review on {by}; re-decide it (fresh decided_on and review_by, justification re-checked) or remove it and fix the code"
            ),
        ));
    }
}

fn budget(input: &Input<'_>, out: &mut Vec<Finding>) {
    let policy = input.policy;
    let exceptions = &input.record.exceptions;
    if exceptions.len() > policy.max_total {
        out.push(finding(
            RECORD_BUDGET,
            input,
            format!(
                "{} exceptions exceed max_total {}; fix the code behind some entries and remove them",
                exceptions.len(),
                policy.max_total
            ),
        ));
    }
    let mut per_rule: BTreeMap<String, usize> = BTreeMap::new();
    for exception in exceptions {
        *per_rule.entry(normalise_rule(&exception.rule)).or_default() += 1;
    }
    for (rule, count) in per_rule {
        if count > policy.max_per_rule {
            out.push(finding(
                RECORD_BUDGET,
                input,
                format!(
                    "rule `{rule}` has {count} exceptions, over max_per_rule {}; fix the pattern at its source instead of excepting each site",
                    policy.max_per_rule
                ),
            ));
        }
    }
    let broad = exceptions.iter().filter(|e| e.policy).count();
    if broad > policy.max_policy {
        out.push(finding(
            RECORD_BUDGET,
            input,
            format!(
                "{broad} policy-level (glob) exceptions exceed max_policy {}; narrow them to single files",
                policy.max_policy
            ),
        ));
    }
    for tier in Tier::RANKED {
        let count = tier_count(input.record, policy, tier);
        if let Some(cap) = policy.count_cap(tier)
            && count > cap
        {
            out.push(finding(
                RECORD_BUDGET,
                input,
                format!(
                    "{count} exceptions written by the {} tier exceed trust.{}.max_count {cap}",
                    tier.name(),
                    tier.name()
                ),
            ));
        }
    }
    if let Some(ratchet) = &input.record.ratchet
        && ratchet.high_water > policy.max_total
    {
        out.push(finding(
            RECORD_BUDGET,
            input,
            format!(
                "[ratchet] high_water {} exceeds max_total {}",
                ratchet.high_water, policy.max_total
            ),
        ));
    }
}

fn tier_count(record: &Record, policy: &RecordPolicy, tier: Tier) -> usize {
    record
        .exceptions
        .iter()
        .filter(|e| {
            present(e.added_by_model.as_ref()).is_some_and(|model| policy.tier(model) == tier)
        })
        .count()
}

fn ratchet(input: &Input<'_>, out: &mut Vec<Finding>) -> Option<usize> {
    let count = input.record.exceptions.len();
    let Some(ratchet) = &input.record.ratchet else {
        if count > 0 {
            out.push(finding(
                RECORD_RATCHET,
                input,
                format!(
                    "{count} exception(s) but no [ratchet] table; add [ratchet] with high_water = {count}, decided_by (a configured decider), decided_on and reason"
                ),
            ));
        }
        return None;
    };
    let mut problems = Vec::new();
    if !input.policy.is_decider(&ratchet.decided_by) {
        problems.push(format!(
            "decided_by `{}` is not one of the configured deciders",
            ratchet.decided_by
        ));
    }
    if let Some(problem) = justification_problem(&ratchet.reason) {
        problems.push(format!("reason {problem}"));
    }
    if ratchet.decided_on > input.today {
        problems.push(format!(
            "decided_on {} is in the future",
            ratchet.decided_on
        ));
    }
    if count > ratchet.high_water {
        problems.push(format!(
            "the record grew to {count} exceptions, past high_water {}; remove entries, or have a configured decider raise high_water with a fresh decided_on and reason",
            ratchet.high_water
        ));
    }
    if let Some(previous) = input.previous {
        let before = ceiling(previous);
        if ratchet.high_water > before {
            let earlier = previous.ratchet.as_ref().map(|p| p.decided_on);
            let newer = earlier.is_none_or(|date| ratchet.decided_on > date);
            let recent = ratchet.decided_on >= input.today.add_days(-RAISE_WINDOW_DAYS);
            if !(newer && recent) {
                let since = earlier.map(|date| format!(" ({date})")).unwrap_or_default();
                problems.push(format!(
                    "high_water raised from {before} to {} without a fresh signature: decided_on must be newer than the previous signature{since} and within the last {RAISE_WINDOW_DAYS} days",
                    ratchet.high_water
                ));
            }
        }
    }
    if !problems.is_empty() {
        out.push(finding(
            RECORD_RATCHET,
            input,
            format!("[ratchet] {}", problems.join("; ")),
        ));
    }
    (count < ratchet.high_water).then_some(count)
}

/// Counts against budgets, per tier, and the entries most in need of attention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// Number of exceptions.
    pub total: usize,
    /// Committed ceiling, when the record has a ratchet.
    pub high_water: Option<usize>,
    /// Configured total cap.
    pub max_total: usize,
    /// Largest count for one rule id.
    pub busiest_rule: usize,
    /// Configured per-rule cap.
    pub max_per_rule: usize,
    /// Policy-level exceptions.
    pub policy: usize,
    /// Configured policy-level cap.
    pub max_policy: usize,
    /// Count and cap per tier, most trusted first.
    pub tiers: Vec<(Tier, usize, Option<usize>)>,
    /// Entries whose model maps to no tier.
    pub unknown: usize,
    /// Earliest review deadline: date, rule, path.
    pub soonest: Option<(Date, String, String)>,
    /// Earliest decision: date, rule, path.
    pub oldest: Option<(Date, String, String)>,
}

impl Status {
    /// Summarises `record` against `policy`.
    #[must_use]
    pub fn of(record: &Record, policy: &RecordPolicy) -> Self {
        let mut per_rule: BTreeMap<String, usize> = BTreeMap::new();
        for exception in &record.exceptions {
            *per_rule.entry(normalise_rule(&exception.rule)).or_default() += 1;
        }
        let earliest = |date: fn(&Exception) -> Option<Date>| {
            record
                .exceptions
                .iter()
                .filter_map(|e| date(e).map(|d| (d, e.rule.clone(), e.path.clone())))
                .min()
        };
        let counted: usize = Tier::RANKED
            .iter()
            .map(|tier| tier_count(record, policy, *tier))
            .sum();
        Self {
            total: record.exceptions.len(),
            high_water: record.ratchet.as_ref().map(|r| r.high_water),
            max_total: policy.max_total,
            busiest_rule: per_rule.values().copied().max().unwrap_or(0),
            max_per_rule: policy.max_per_rule,
            policy: record.exceptions.iter().filter(|e| e.policy).count(),
            max_policy: policy.max_policy,
            tiers: Tier::RANKED
                .iter()
                .map(|tier| {
                    (
                        *tier,
                        tier_count(record, policy, *tier),
                        policy.count_cap(*tier),
                    )
                })
                .collect(),
            unknown: record.exceptions.len().saturating_sub(counted),
            soonest: earliest(|e| e.review_by),
            oldest: earliest(|e| e.decided_on),
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let high_water = self
            .high_water
            .map_or_else(|| "no ratchet".to_owned(), |h| format!("high water {h}"));
        writeln!(
            f,
            "Record budget: {}/{} exceptions ({high_water}); per rule: max {}/{}; policy-level: {}/{}",
            self.total,
            self.max_total,
            self.busiest_rule,
            self.max_per_rule,
            self.policy,
            self.max_policy
        )?;
        let mut tiers: Vec<String> = self
            .tiers
            .iter()
            .map(|(tier, count, cap)| match cap {
                Some(cap) => format!("{} {count}/{cap}", tier.name()),
                None => format!("{} {count}", tier.name()),
            })
            .collect();
        if self.unknown > 0 {
            tiers.push(format!("unknown {}", self.unknown));
        }
        write!(f, "By trust tier: {}", tiers.join(", "))?;
        if let Some((date, rule, path)) = &self.soonest {
            write!(f, "\nsoonest review: {date} `{rule}` at `{path}`")?;
        }
        if let Some((date, rule, path)) = &self.oldest {
            write!(f, "\noldest decision: {date} `{rule}` at `{path}`")?;
        }
        Ok(())
    }
}

/// Rewrites the `high_water` value inside the `[ratchet]` table, keeping layout and comments.
/// Returns `None` when the file has no such line.
#[must_use]
pub fn set_high_water(text: &str, value: usize) -> Option<String> {
    let mut table = "";
    let mut done = false;
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            table = trimmed;
        } else if !done
            && table == "[ratchet]"
            && trimmed
                .strip_prefix("high_water")
                .is_some_and(|rest| rest.trim_start().starts_with('='))
            && let Some((key, rest)) = line.split_once('=')
        {
            let value_text = rest.trim_start();
            let spacing = rest.get(..rest.len() - value_text.len()).unwrap_or(" ");
            let tail = value_text.trim_start_matches(|c: char| c.is_ascii_digit() || c == '_');
            let _ = write!(out, "{key}={spacing}{value}{tail}");
            done = true;
            continue;
        }
        out.push_str(line);
    }
    done.then_some(out)
}

/// Removes the exceptions at `indexes` (file order), each with the comment lines directly above
/// its `[[exception]]` header.
#[must_use]
pub fn remove_exceptions(text: &str, indexes: &[usize]) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let is_comment = |i: usize| lines.get(i).is_some_and(|l| l.trim().starts_with('#'));
    let headers: Vec<usize> = (0..lines.len())
        .filter(|i| lines.get(*i).is_some_and(|l| l.trim().starts_with('[')))
        .collect();
    let blocks: Vec<usize> = headers
        .iter()
        .copied()
        .filter(|i| lines.get(*i).is_some_and(|l| l.trim() == "[[exception]]"))
        .collect();
    let mut drop = vec![false; lines.len()];
    for index in indexes {
        let Some(&header) = blocks.get(*index) else {
            continue;
        };
        let mut start = header;
        while start > 0 && is_comment(start - 1) {
            start -= 1;
        }
        let mut end = headers
            .iter()
            .copied()
            .find(|h| *h > header)
            .unwrap_or(lines.len());
        while end > header + 1 && is_comment(end - 1) {
            end -= 1;
        }
        for slot in drop.iter_mut().take(end).skip(start) {
            *slot = true;
        }
    }
    lines
        .iter()
        .zip(drop)
        .filter_map(|(line, dropped)| (!dropped).then_some(*line))
        .collect()
}

/// Result of the record gate.
#[derive(Debug, Clone)]
pub struct GateReport {
    /// Structural and policy findings.
    pub findings: Vec<Finding>,
    /// Counts against budgets after any lowering.
    pub status: Status,
    /// Informational lines (lowered ceiling, unavailable history).
    pub notes: Vec<String>,
}

/// Runs every record control for the workspace at `root`: structure, policy and the ratchet.
/// When `may_lower` is set and the record shrank, `high_water` is rewritten to the new count;
/// otherwise the unlowered ceiling is a finding.
///
/// # Errors
/// Returns an error when the record cannot be read, parsed or rewritten.
pub fn gate(
    root: &Path,
    policy: &RecordPolicy,
    today: Date,
    may_lower: bool,
) -> Result<GateReport, Error> {
    let file = root.join(&policy.path);
    let mut record = Record::load(&file)?;
    let mut findings = record.validate(&policy.path);
    let mut notes = Vec::new();
    let previous = previous_record(root, &policy.path, &policy.history_base);
    if previous.is_none() && !record.exceptions.is_empty() {
        notes.push(
            "no committed history for the record; a raise of high_water is not checked against it"
                .to_owned(),
        );
    }
    let verdict = review(&Input {
        record: &record,
        record_path: &policy.path,
        policy,
        today,
        previous: previous.as_ref(),
    });
    findings.extend(verdict.findings);
    if let Some(count) = verdict.lower_to {
        let rewritten = if may_lower {
            set_high_water(&crate::read_to_string(&file)?, count)
        } else {
            None
        };
        if let Some(text) = rewritten {
            std::fs::write(&file, text).map_err(|source| Error::Write {
                path: file.clone(),
                source,
            })?;
            if let Some(ratchet) = record.ratchet.as_mut() {
                ratchet.high_water = count;
            }
            notes.push(format!(
                "the record shrank: lowered [ratchet] high_water to {count}; commit the change"
            ));
        } else {
            findings.push(Finding {
                rule: RECORD_RATCHET,
                path: policy.path.clone(),
                line: 0,
                message: format!(
                    "[ratchet] high_water {} is above the {count} exception(s) the record holds; lower it to {count} (a local `rust-qa run` does this automatically)",
                    ceiling(&record)
                ),
            });
        }
    }
    Ok(GateReport {
        findings,
        status: Status::of(&record, policy),
        notes,
    })
}

/// The record as committed before this change: the lower ceiling of `HEAD` and the merge base
/// with `base`. `None` when git or the file's history is unavailable.
#[must_use]
pub fn previous_record(root: &Path, record_path: &Path, base: &str) -> Option<Record> {
    let git = |args: &[&str]| -> Option<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    };
    let rel = record_path.to_string_lossy().replace('\\', "/");
    let mut revisions = vec!["HEAD".to_owned()];
    if let Some(merge_base) = git(&["merge-base", base, "HEAD"]) {
        revisions.push(merge_base.trim().to_owned());
    }
    revisions
        .iter()
        .filter_map(|rev| git(&["show", &format!("{rev}:{rel}")]))
        .filter_map(|text| toml::from_str::<Record>(&text).ok())
        .min_by_key(ceiling)
}
