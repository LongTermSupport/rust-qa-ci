//! `rqaci_sensitive_debug_derive`: a derived `Debug` that would print a sensitive value.
//!
//! Rule doc: `crates/rust-qa/docs/rules/rqaci_sensitive_debug_derive.md`.

use clippy_utils::diagnostics::span_lint_hir_and_then;
use rustc_hir::{Item, ItemKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::{self, AdtDef, AdtKind, Ty, TyCtxt};
use rustc_span::def_id::DefId;
use rustc_span::{Symbol, sym};
use serde::Deserialize;

rustc_session::declare_lint! {
    /// ### What it does
    ///
    /// Flags `#[derive(Debug)]` on a local struct or enum that would print a sensitive value:
    /// either the type's own name matches a sensitive pattern and it holds data, or one of its
    /// fields resolves (through aliases, references, tuples, arrays and generic arguments such
    /// as `Option<Vec<_>>`) to a sensitive type whose own `Debug` is also derived.
    ///
    /// ### Why is this bad?
    ///
    /// Derived `Debug` prints every field verbatim, so secrets end up in logs, panics and
    /// error reports.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// #[derive(Debug)]
    /// struct Client { token: ApiToken }
    /// ```
    ///
    /// Use instead: a hand-written `Debug` that redacts the field.
    pub RQACI_SENSITIVE_DEBUG_DERIVE,
    Deny,
    "derived `Debug` would print a sensitive value"
}

rustc_session::impl_lint_pass!(SensitiveDebugDerive => [RQACI_SENSITIVE_DEBUG_DERIVE]);

/// `dylint.toml` table holding this lint's configuration.
const CONFIG_TABLE: &str = "rqaci_sensitive_debug_derive";

const DEFAULT_PATTERNS: [&str; 7] = [
    "Secret",
    "Password",
    "Passwd",
    "Token",
    "ApiKey",
    "Credential",
    "PrivateKey",
];

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Case-insensitive substrings matched against a type's simple name. Replaces the defaults.
    sensitive_type_patterns: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sensitive_type_patterns: DEFAULT_PATTERNS.iter().map(ToString::to_string).collect(),
        }
    }
}

struct Pattern {
    configured: String,
    lowercase: String,
}

pub struct SensitiveDebugDerive {
    patterns: Vec<Pattern>,
}

impl SensitiveDebugDerive {
    pub fn from_config() -> Self {
        let config: Config = dylint_linting::config_or_default(CONFIG_TABLE);
        let patterns = config
            .sensitive_type_patterns
            .into_iter()
            .filter(|pattern| !pattern.is_empty())
            .map(|configured| Pattern {
                lowercase: configured.to_lowercase(),
                configured,
            })
            .collect();
        Self { patterns }
    }

    fn matching_pattern(&self, type_name: Symbol) -> Option<&str> {
        let name = type_name.as_str().to_lowercase();
        self.patterns
            .iter()
            .find(|pattern| name.contains(&pattern.lowercase))
            .map(|pattern| pattern.configured.as_str())
    }

    /// Sensitive and printable: the name matches, it holds data, and its `Debug` is derived.
    /// A hand-written `Debug` is trusted to redact, which is the fix this lint asks for.
    fn leaks_through_debug(&self, tcx: TyCtxt<'_>, debug: DefId, adt: AdtDef<'_>) -> Option<&str> {
        let pattern = self.matching_pattern(tcx.item_name(adt.did()))?;
        let holds_data = adt.all_fields().next().is_some();
        let adt_ty = tcx
            .type_of(adt.did())
            .instantiate_identity()
            .skip_norm_wip();
        let derived = tcx
            .non_blanket_impls_for_ty(debug, adt_ty)
            .any(|impl_id| tcx.is_automatically_derived(impl_id));
        (holds_data && derived).then_some(pattern)
    }

    /// First sensitive type reachable from `ty` through its generic arguments.
    fn leaking_type<'tcx>(
        &self,
        tcx: TyCtxt<'tcx>,
        debug: DefId,
        ty: Ty<'tcx>,
    ) -> Option<(Ty<'tcx>, &str)> {
        ty.walk().filter_map(|arg| arg.as_type()).find_map(|inner| {
            let ty::Adt(adt, _) = inner.kind() else {
                return None;
            };
            self.leaks_through_debug(tcx, debug, *adt)
                .map(|pattern| (inner, pattern))
        })
    }
}

impl<'tcx> LateLintPass<'tcx> for SensitiveDebugDerive {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        let ItemKind::Impl(_) = item.kind else {
            return;
        };
        let tcx = cx.tcx;
        let impl_id = item.owner_id.to_def_id();
        let Some(debug) = tcx.get_diagnostic_item(sym::Debug) else {
            return;
        };
        if !tcx.is_automatically_derived(impl_id) || tcx.impl_opt_trait_id(impl_id) != Some(debug) {
            return;
        }
        let self_ty = tcx.type_of(impl_id).instantiate_identity().skip_norm_wip();
        let ty::Adt(adt, args) = self_ty.kind() else {
            return;
        };
        let Some(adt_local_id) = adt.did().as_local() else {
            return;
        };

        // A sensitive type that derives `Debug` leaks all of its contents; one report covers it.
        if let Some(pattern) = self.leaks_through_debug(tcx, debug, *adt) {
            let name = tcx.item_name(adt.did());
            span_lint_hir_and_then(
                cx,
                RQACI_SENSITIVE_DEBUG_DERIVE,
                tcx.local_def_id_to_hir_id(adt_local_id),
                tcx.def_span(adt.did()),
                format!("derived `Debug` would print the contents of sensitive type `{name}`"),
                |diag| {
                    diag.note(format!(
                        "`{name}` matches the sensitive type pattern `{pattern}`"
                    ));
                    diag.help("implement `Debug` by hand and redact the contents");
                },
            );
            return;
        }

        for variant in adt.variants() {
            for field in &variant.fields {
                let Some(field_local_id) = field.did.as_local() else {
                    continue;
                };
                let field_ty = field.ty(tcx, args).skip_norm_wip();
                let Some((sensitive_ty, pattern)) = self.leaking_type(tcx, debug, field_ty) else {
                    continue;
                };
                let field_name = match adt.adt_kind() {
                    AdtKind::Enum => format!("{}.{}", variant.name, field.name),
                    AdtKind::Struct | AdtKind::Union => field.name.to_string(),
                };
                span_lint_hir_and_then(
                    cx,
                    RQACI_SENSITIVE_DEBUG_DERIVE,
                    tcx.local_def_id_to_hir_id(field_local_id),
                    tcx.def_span(field.did),
                    format!(
                        "derived `Debug` would print sensitive field `{field_name}` of type `{field_ty}`"
                    ),
                    |diag| {
                        diag.note(format!(
                            "`{sensitive_ty}` matches the sensitive type pattern `{pattern}` and derives `Debug`"
                        ));
                        diag.help("implement `Debug` by hand and redact the field");
                    },
                );
            }
        }
    }
}
