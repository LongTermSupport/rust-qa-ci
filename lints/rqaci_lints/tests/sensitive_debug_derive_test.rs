//! Red-proof UI fixtures for `rqaci_sensitive_debug_derive`.

#[test]
fn ui_default_patterns() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui/sensitive_debug_derive");
}

#[test]
fn ui_configured_patterns() {
    dylint_testing::ui::Test::src_base(env!("CARGO_PKG_NAME"), "ui_config/sensitive_debug_derive")
        .dylint_toml(concat!(
            "[rqaci_sensitive_debug_derive]\n",
            "sensitive_type_patterns = [\"Nonce\"]\n",
            "redacting_types = [\"vault::Sealed\"]\n",
        ))
        .run();
}
