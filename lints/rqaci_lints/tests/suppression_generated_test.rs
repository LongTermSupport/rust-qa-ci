//! Red-proof UI fixtures for `rqaci_suppression_generated`.

#[test]
fn ui_generated_suppressions() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui/suppression_generated");
}
