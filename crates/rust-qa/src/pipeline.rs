//! Phase ordering.
//!
//! Steps run in catalogue order: format, then detectors, then runners; within a phase the built-in
//! tools come first, in catalogue order, then the project's own stages, in the order the
//! configuration declares them. Every selected detector runs (unless `fail_fast`), so one invocation
//! reports all static findings; any failure in the format or detector phase stops the run before the
//! first runner. Runners stop at the first failure.

use serde::Deserialize;

use crate::tools::Tool;

/// Pipeline phase, in execution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
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

/// One unit of a run: a built-in tool, or a project stage declared in the configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    /// A built-in tool.
    Tool(Tool),
    /// The project stage at `index` in the configuration's `[[stage]]` list.
    Stage {
        /// Position in the configuration.
        index: usize,
        /// Phase the stage declares.
        phase: Phase,
    },
}

impl Step {
    /// Phase the step belongs to.
    #[must_use]
    pub fn phase(self) -> Phase {
        match self {
            Self::Tool(tool) => tool.phase(),
            Self::Stage { phase, .. } => phase,
        }
    }

    fn order(self) -> (Phase, usize, usize) {
        match self {
            Self::Tool(tool) => (
                tool.phase(),
                0,
                Tool::ALL.iter().position(|t| *t == tool).unwrap_or(0),
            ),
            Self::Stage { index, phase } => (phase, 1, index),
        }
    }
}

impl PartialOrd for Step {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Step {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.order().cmp(&other.order())
    }
}

/// Result of one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The step passed.
    Passed,
    /// The step failed; its own output explains why.
    Failed,
    /// The step does not apply to this workspace (reason given).
    Skipped(String),
}

/// Runs one step.
pub trait Executor {
    /// Runs `step` and reports its outcome.
    fn run(&mut self, step: Step) -> Outcome;
}

/// Outcome of a pipeline run.
#[derive(Debug, Clone)]
pub struct Report {
    /// Steps that ran, in order, with outcomes.
    pub results: Vec<(Step, Outcome)>,
    /// Whether a format or detector failure prevented the runners.
    pub stopped_before_runners: bool,
    /// Whether every step that ran passed or was skipped.
    pub passed: bool,
}

/// Runs `selected` steps in catalogue order with the stop rules described in the module docs.
pub fn execute(selected: &[Step], executor: &mut dyn Executor, fail_fast: bool) -> Report {
    let mut steps = selected.to_vec();
    steps.sort();
    steps.dedup();
    let mut report = Report {
        results: Vec::new(),
        stopped_before_runners: false,
        passed: true,
    };
    for step in steps {
        if !report.passed && (fail_fast || step.phase() == Phase::Runners) {
            report.stopped_before_runners = report
                .results
                .iter()
                .all(|(ran, _)| ran.phase() != Phase::Runners);
            break;
        }
        let outcome = executor.run(step);
        if outcome == Outcome::Failed {
            report.passed = false;
        }
        report.results.push((step, outcome));
    }
    report
}
