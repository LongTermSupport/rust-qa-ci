//! Phase ordering.
//!
//! Tools run in catalogue order: format, then detectors, then runners. Every selected detector runs
//! (unless `fail_fast`), so one invocation reports all static findings; any failure in the format or
//! detector phase stops the run before the first runner. Runners stop at the first failure.

use crate::tools::Tool;

/// Pipeline phase, in execution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    /// Formatting (modifies locally, checks in CI).
    Format,
    /// Static detectors: read code, never execute it.
    Detectors,
    /// Runners: execute code or audit dependencies.
    Runners,
}

impl Phase {
    /// Lower-case name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Format => "format",
            Self::Detectors => "detectors",
            Self::Runners => "runners",
        }
    }
}

/// Result of one tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The tool passed.
    Passed,
    /// The tool failed; its own output explains why.
    Failed,
    /// The tool does not apply to this workspace (reason given).
    Skipped(String),
}

/// Runs one tool.
pub trait Executor {
    /// Runs `tool` and reports its outcome.
    fn run(&mut self, tool: Tool) -> Outcome;
}

/// Outcome of a pipeline run.
#[derive(Debug, Clone)]
pub struct Report {
    /// Tools that ran, in order, with outcomes.
    pub results: Vec<(Tool, Outcome)>,
    /// Whether a format or detector failure prevented the runners.
    pub stopped_before_runners: bool,
    /// Whether every tool that ran passed or was skipped.
    pub passed: bool,
}

/// Runs `selected` tools in catalogue order with the stop rules described in the module docs.
pub fn execute(selected: &[Tool], executor: &mut dyn Executor, fail_fast: bool) -> Report {
    let mut tools = selected.to_vec();
    tools.sort();
    tools.dedup();
    let mut report = Report {
        results: Vec::new(),
        stopped_before_runners: false,
        passed: true,
    };
    for tool in tools {
        if !report.passed && (fail_fast || tool.phase() == Phase::Runners) {
            report.stopped_before_runners = report
                .results
                .iter()
                .all(|(ran, _)| ran.phase() != Phase::Runners);
            break;
        }
        let outcome = executor.run(tool);
        if outcome == Outcome::Failed {
            report.passed = false;
        }
        report.results.push((tool, outcome));
    }
    report
}
