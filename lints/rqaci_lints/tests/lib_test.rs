//! Library smoke test: the library builds, loads into the driver and stays silent on clean code.

#[test]
fn ui_clean() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui/clean");
}
