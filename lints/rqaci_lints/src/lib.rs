#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

mod sensitive_debug_derive;

dylint_linting::dylint_library!();

#[unsafe(no_mangle)]
pub fn register_lints(sess: &rustc_session::Session, lint_store: &mut rustc_lint::LintStore) {
    // Must run before any lint reads its `dylint.toml` table.
    dylint_linting::init_config(sess);

    lint_store.register_lints(&[sensitive_debug_derive::RQACI_SENSITIVE_DEBUG_DERIVE]);
    lint_store.register_late_lint_pass(Box::new(|_| {
        Box::new(sensitive_debug_derive::SensitiveDebugDerive::from_config())
    }));
}
