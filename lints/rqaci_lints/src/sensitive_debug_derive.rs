//! `rqaci_sensitive_debug_derive`: a derived `Debug` that would print a sensitive value.
//!
//! Rule doc: `crates/rust-qa/docs/rules/rqaci_sensitive_debug_derive.md`.

use clippy_utils::diagnostics::span_lint_hir_and_then;
use rustc_hir::{Item, ItemKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::print::with_no_trimmed_paths;
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
    /// as `Option<Vec<_>>`) to a sensitive type whose own `Debug` is also derived. Fields that
    /// are configured redacting types (default: `secrecy::SecretBox`) do not count as data.
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

/// `SecretString` is an alias of `SecretBox<str>`; the alias is listed for readers, the resolved
/// `SecretBox` entry is what matches.
const DEFAULT_REDACTING_TYPES: [&str; 2] = ["secrecy::SecretBox", "secrecy::SecretString"];

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Case-insensitive substrings matched against a type's simple name. Replaces the defaults.
    sensitive_type_patterns: Vec<String>,
    /// Def paths of types whose `Debug` redacts. Replaces the defaults.
    redacting_types: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sensitive_type_patterns: DEFAULT_PATTERNS.iter().map(ToString::to_string).collect(),
            redacting_types: DEFAULT_REDACTING_TYPES
                .iter()
                .map(ToString::to_string)
                .collect(),
        }
    }
}

struct Pattern {
    configured: String,
    lowercase: String,
}

pub struct SensitiveDebugDerive {
    patterns: Vec<Pattern>,
    redacting_types: Vec<String>,
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
        Self {
            patterns,
            redacting_types: config.redacting_types,
        }
    }

    /// A configured entry matches the full def path or a suffix of it on a `::` boundary, so
    /// `secrecy::SecretBox` also matches a crate-local `crate::secrecy::SecretBox`.
    fn is_redacting_adt(&self, tcx: TyCtxt<'_>, did: DefId) -> bool {
        let path = with_no_trimmed_paths!(tcx.def_path_str(did));
        self.redacting_types.iter().any(|configured| {
            path == *configured
                || path
                    .strip_suffix(configured.as_str())
                    .is_some_and(|prefix| prefix.ends_with("::"))
        })
    }

    /// Whether `Debug` on `ty` can only print redacted output: a redacting type, possibly behind
    /// references, arrays, slices, tuples, `Box`, `Option`, `Vec`, `Rc` or `Arc`.
    fn prints_only_redacted<'tcx>(&self, tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> bool {
        match ty.kind() {
            ty::Adt(adt, _) if self.is_redacting_adt(tcx, adt.did()) => true,
            ty::Adt(adt, args) if adt.is_box() || is_transparent_container(tcx, adt.did()) => {
                self.prints_only_redacted(tcx, args.type_at(0))
            }
            ty::Ref(_, inner, _) | ty::Array(inner, _) | ty::Slice(inner) => {
                self.prints_only_redacted(tcx, *inner)
            }
            ty::Tuple(elems) => {
                !elems.is_empty()
                    && elems
                        .iter()
                        .all(|elem| self.prints_only_redacted(tcx, elem))
            }
            _ => false,
        }
    }

    fn matching_pattern(&self, type_name: Symbol) -> Option<&str> {
        let name = type_name.as_str().to_lowercase();
        self.patterns
            .iter()
            .find(|pattern| name.contains(&pattern.lowercase))
            .map(|pattern| pattern.configured.as_str())
    }

    /// Sensitive and printable: the name matches, its `Debug` is derived, and it holds at least
    /// one field that is not a redacting type. A hand-written `Debug` is trusted to redact, which
    /// is the fix this lint asks for.
    fn leaks_through_debug(&self, tcx: TyCtxt<'_>, debug: DefId, adt: AdtDef<'_>) -> Option<&str> {
        let pattern = self.matching_pattern(tcx.item_name(adt.did()))?;
        let prints_clear_data = adt.all_fields().any(|field| {
            let field_ty = tcx
                .type_of(field.did)
                .instantiate_identity()
                .skip_norm_wip();
            !self.prints_only_redacted(tcx, field_ty)
        });
        let adt_ty = tcx
            .type_of(adt.did())
            .instantiate_identity()
            .skip_norm_wip();
        let derived = tcx
            .non_blanket_impls_for_ty(debug, adt_ty)
            .any(|impl_id| tcx.is_automatically_derived(impl_id));
        (prints_clear_data && derived).then_some(pattern)
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

/// Std containers whose derived-style `Debug` prints only their element type's `Debug`.
fn is_transparent_container(tcx: TyCtxt<'_>, did: DefId) -> bool {
    [sym::Option, sym::Vec, sym::Rc, sym::Arc]
        .into_iter()
        .any(|item| tcx.is_diagnostic_item(item, did))
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
