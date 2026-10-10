//! Phase ordering: detectors run before runners, and a detector failure stops the run.
#![cfg(test)]

use rust_qa::pipeline::{Executor, Outcome, Phase, Step, execute};
use rust_qa::tools::Tool;

struct Fake {
    failing: Vec<Step>,
    ran: Vec<Step>,
}

impl Executor for Fake {
    fn run(&mut self, step: Step) -> Outcome {
        self.ran.push(step);
        if self.failing.contains(&step) {
            Outcome::Failed
        } else {
            Outcome::Passed
        }
    }
}

fn fake(failing: &[Step]) -> Fake {
    Fake {
        failing: failing.to_vec(),
        ran: Vec::new(),
    }
}

const FMT: Step = Step::Tool(Tool::Fmt);
const SUPPRESSION: Step = Step::Tool(Tool::Suppression);
const CLIPPY: Step = Step::Tool(Tool::Clippy);
const DYLINT: Step = Step::Tool(Tool::Dylint);
const TEST: Step = Step::Tool(Tool::Test);
const COVERAGE: Step = Step::Tool(Tool::Coverage);

const SELECTED: [Step; 6] = [FMT, SUPPRESSION, CLIPPY, DYLINT, TEST, COVERAGE];

#[test]
fn everything_runs_in_order_when_all_pass() {
    let mut exec = fake(&[]);
    let report = execute(&SELECTED, &mut exec, false);
    assert!(report.passed);
    assert_eq!(exec.ran, SELECTED.to_vec());
}

#[test]
fn detector_failure_runs_remaining_detectors_but_no_runner() {
    let mut exec = fake(&[SUPPRESSION]);
    let report = execute(&SELECTED, &mut exec, false);
    assert!(!report.passed);
    assert!(report.stopped_before_runners);
    assert_eq!(exec.ran, vec![FMT, SUPPRESSION, CLIPPY, DYLINT]);
}

#[test]
fn fail_fast_stops_at_the_first_failure() {
    let mut exec = fake(&[SUPPRESSION]);
    let report = execute(&SELECTED, &mut exec, true);
    assert!(!report.passed);
    assert_eq!(exec.ran, vec![FMT, SUPPRESSION]);
}

#[test]
fn format_failure_still_runs_detectors_but_no_runner() {
    let mut exec = fake(&[FMT]);
    let report = execute(&SELECTED, &mut exec, false);
    assert!(report.stopped_before_runners);
    assert_eq!(exec.ran, vec![FMT, SUPPRESSION, CLIPPY, DYLINT]);
}

#[test]
fn runner_failure_stops_later_runners() {
    let mut exec = fake(&[TEST]);
    let report = execute(&SELECTED, &mut exec, false);
    assert!(!report.passed);
    assert!(!report.stopped_before_runners);
    assert_eq!(exec.ran.last(), Some(&TEST));
}

#[test]
fn skipped_tools_do_not_fail_the_run() {
    struct Skipper;
    impl Executor for Skipper {
        fn run(&mut self, _step: Step) -> Outcome {
            Outcome::Skipped("not configured".to_owned())
        }
    }
    let report = execute(&SELECTED, &mut Skipper, false);
    assert!(report.passed);
    assert_eq!(report.results.len(), SELECTED.len());
}

#[test]
fn input_order_does_not_matter() {
    let mut exec = fake(&[]);
    execute(&[TEST, CLIPPY, FMT], &mut exec, false);
    assert_eq!(exec.ran, vec![FMT, CLIPPY, TEST]);
}

#[test]
fn project_stages_run_in_their_phase_after_the_built_in_tools_of_that_phase() {
    let check = Step::Stage {
        index: 1,
        phase: Phase::Format,
    };
    let lint = Step::Stage {
        index: 0,
        phase: Phase::Detectors,
    };
    let suite = Step::Stage {
        index: 2,
        phase: Phase::Runners,
    };
    let mut exec = fake(&[]);
    execute(&[suite, TEST, lint, CLIPPY, check, FMT], &mut exec, false);
    assert_eq!(exec.ran, vec![FMT, check, CLIPPY, lint, TEST, suite]);
}

#[test]
fn a_failing_detector_stage_stops_the_runners() {
    let lint = Step::Stage {
        index: 0,
        phase: Phase::Detectors,
    };
    let mut exec = fake(&[lint]);
    let report = execute(&[lint, CLIPPY, TEST], &mut exec, false);
    assert!(report.stopped_before_runners);
    assert_eq!(exec.ran, vec![CLIPPY, lint]);
}
