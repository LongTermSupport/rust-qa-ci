//! Phase ordering: detectors run before runners, and a detector failure stops the run.
#![cfg(test)]

use rust_qa::pipeline::{Executor, Outcome, execute};
use rust_qa::tools::Tool;

struct Fake {
    failing: Vec<Tool>,
    ran: Vec<Tool>,
}

impl Executor for Fake {
    fn run(&mut self, tool: Tool) -> Outcome {
        self.ran.push(tool);
        if self.failing.contains(&tool) {
            Outcome::Failed
        } else {
            Outcome::Passed
        }
    }
}

fn fake(failing: &[Tool]) -> Fake {
    Fake {
        failing: failing.to_vec(),
        ran: Vec::new(),
    }
}

const SELECTED: [Tool; 6] = [Tool::Fmt, Tool::Suppression, Tool::Clippy, Tool::Dylint, Tool::Test, Tool::Coverage];

#[test]
fn everything_runs_in_order_when_all_pass() {
    let mut exec = fake(&[]);
    let report = execute(&SELECTED, &mut exec, false);
    assert!(report.passed);
    assert_eq!(exec.ran, SELECTED.to_vec());
}

#[test]
fn detector_failure_runs_remaining_detectors_but_no_runner() {
    let mut exec = fake(&[Tool::Suppression]);
    let report = execute(&SELECTED, &mut exec, false);
    assert!(!report.passed);
    assert!(report.stopped_before_runners);
    assert_eq!(exec.ran, vec![Tool::Fmt, Tool::Suppression, Tool::Clippy, Tool::Dylint]);
}

#[test]
fn fail_fast_stops_at_the_first_failure() {
    let mut exec = fake(&[Tool::Suppression]);
    let report = execute(&SELECTED, &mut exec, true);
    assert!(!report.passed);
    assert_eq!(exec.ran, vec![Tool::Fmt, Tool::Suppression]);
}

#[test]
fn format_failure_still_runs_detectors_but_no_runner() {
    let mut exec = fake(&[Tool::Fmt]);
    let report = execute(&SELECTED, &mut exec, false);
    assert!(report.stopped_before_runners);
    assert_eq!(exec.ran, vec![Tool::Fmt, Tool::Suppression, Tool::Clippy, Tool::Dylint]);
}

#[test]
fn runner_failure_stops_later_runners() {
    let mut exec = fake(&[Tool::Test]);
    let report = execute(&SELECTED, &mut exec, false);
    assert!(!report.passed);
    assert!(!report.stopped_before_runners);
    assert_eq!(exec.ran.last(), Some(&Tool::Test));
}

#[test]
fn skipped_tools_do_not_fail_the_run() {
    struct Skipper;
    impl Executor for Skipper {
        fn run(&mut self, _tool: Tool) -> Outcome {
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
    execute(&[Tool::Test, Tool::Clippy, Tool::Fmt], &mut exec, false);
    assert_eq!(exec.ran, vec![Tool::Fmt, Tool::Clippy, Tool::Test]);
}
