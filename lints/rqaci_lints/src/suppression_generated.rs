//! `rqaci_suppression_generated`: a lint-level attribute produced by macro expansion that
//! silences code the caller wrote.
//!
//! Rule doc: `crates/rust-qa/docs/rules/rqaci_suppression_generated.md`.

use std::ops::ControlFlow;
use std::path::PathBuf;

use clippy_utils::diagnostics::span_lint_hir_and_then;
use rustc_ast::MetaItemInner;
use rustc_hir::attrs::Attribute;
use rustc_hir::intravisit::{self, Visitor};
use rustc_hir::{
    Expr, FieldDef, ForeignItem, HirId, ImplItem, Item, Node, OwnerId, TraitItem, Variant,
};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::hir::nested_filter;
use rustc_middle::ty::TyCtxt;
use rustc_span::source_map::SourceMap;
use rustc_span::{ExpnKind, FileName, RemapPathScopeComponents, Span, sym};

rustc_session::declare_lint! {
    /// ### What it does
    ///
    /// Flags an `allow` or `expect` lint attribute that exists only after macro expansion (a
    /// `macro_rules!` body that builds it from fragments, a caller-supplied level or lint
    /// name, or a procedural macro) and that covers code the caller wrote. Attributes written
    /// literally in source are left to the syntactic suppression guard
    /// (`rqaci.suppression.unrecorded`), which already requires a project-record exception for
    /// them; attributes that cover only generated code are not reported.
    ///
    /// ### Why is this bad?
    ///
    /// A suppression that is assembled by a macro is invisible to any source scan, so it
    /// silences a defence with no recorded decision.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// macro_rules! quiet { ($level:ident, $($item:tt)*) => { #[$level(clippy::unwrap_used)] $($item)* } }
    /// quiet!(allow, fn f(v: Option<u8>) -> u8 { v.unwrap() });
    /// ```
    ///
    /// Use instead: fix the code so the lint passes, or write the suppression literally (and
    /// record it).
    pub RQACI_SUPPRESSION_GENERATED,
    Deny,
    "macro-generated lint suppression over caller-written code"
}

rustc_session::declare_lint_pass!(SuppressionGenerated => [RQACI_SUPPRESSION_GENERATED]);

impl<'tcx> LateLintPass<'tcx> for SuppressionGenerated {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        for owner in cx.tcx.hir_crate_items(()).owners() {
            check_owner(cx, owner);
        }
    }
}

fn check_owner(cx: &LateContext<'_>, owner: OwnerId) {
    let tcx = cx.tcx;
    let source_map = tcx.sess.source_map();
    for (local_id, attrs) in tcx.hir_attr_map(owner).map.iter() {
        let hir_id = HirId {
            owner,
            local_id: *local_id,
        };
        for attr in attrs.iter() {
            let Some(lints) = generated_suppression(source_map, attr) else {
                continue;
            };
            if expanded_from_workspace_macro(tcx, attr.span()) && covers_written_code(tcx, hir_id)
            {
                report(cx, hir_id, attr, &lints);
            }
        }
    }
}

/// Whether the macro that produced `span` is defined in a source file under the build's working
/// directory (the Cargo workspace root): a local `macro_rules!` or a workspace proc-macro crate.
/// Compiler desugarings, the standard library and dependency macros (registry, git) are the
/// dependency author's code, outside the project's suppression policy, as their own lint levels
/// are.
fn expanded_from_workspace_macro(tcx: TyCtxt<'_>, span: Span) -> bool {
    let data = span.ctxt().outer_expn_data();
    if !matches!(data.kind, ExpnKind::Macro(..)) {
        return false;
    }
    let Some(def_id) = data.macro_def_id else {
        return false;
    };
    if def_id.is_local() {
        return true;
    }
    let source_map = tcx.sess.source_map();
    let FileName::Real(file) = source_map.span_to_filename(tcx.def_span(def_id)) else {
        return false;
    };
    let scope = RemapPathScopeComponents::DIAGNOSTICS;
    let (file_dir, file_name) = file.embeddable_name(scope);
    let definition = file_dir.join(file_name);
    let root = source_map.working_dir().path(scope);
    let outside = |dir: Option<std::ffi::OsString>| {
        dir.is_some_and(|dir| definition.starts_with(PathBuf::from(dir)))
    };
    definition.starts_with(root)
        && !outside(std::env::var_os("CARGO_HOME"))
        && !outside(std::env::var_os("CARGO_TARGET_DIR"))
        && !definition.starts_with(root.join("target"))
}

/// The lint names an `allow`/`expect` attribute silences, when at least one of them was not
/// written literally in source (and is therefore invisible to the syntactic guard).
fn generated_suppression(source_map: &SourceMap, attr: &Attribute) -> Option<Vec<String>> {
    let name = attr.name()?;
    if name != sym::allow && name != sym::expect {
        return None;
    }
    let items = attr.meta_item_list()?;
    let mut lints = Vec::new();
    let mut generated = false;
    for item in &items {
        let MetaItemInner::MetaItem(meta) = item else {
            continue;
        };
        if !meta.is_word() {
            continue;
        }
        let path: Vec<String> = meta
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        lints.push(path.join("::"));
        if !written_literally(source_map, attr.span(), item.span()) {
            generated = true;
        }
    }
    (generated && !lints.is_empty()).then_some(lints)
}

/// Whether the attribute is spelled out, level and lint name together, in a source file of this
/// crate: an outer or inner `allow(lint)`/`expect(lint)` attribute, or a `cfg_attr` around one,
/// with no macro metavariable in it. That is exactly the text a syntactic scan can read.
fn written_literally(source_map: &SourceMap, attr_span: Span, lint_span: Span) -> bool {
    if !attr_span.contains(lint_span) || source_map.is_imported(attr_span) {
        return false;
    }
    let Ok(snippet) = source_map.span_to_snippet(attr_span) else {
        return false;
    };
    if snippet.contains('$') {
        return false;
    }
    let body =
        snippet.trim_start_matches(|c: char| c == '#' || c == '!' || c == '[' || c.is_whitespace());
    ["allow", "expect", "cfg_attr"].iter().any(|keyword| {
        body.strip_prefix(keyword)
            .is_some_and(|rest| rest.trim_start().starts_with('('))
    })
}

/// Whether the node an attribute sits on contains anything the caller wrote: an item, field,
/// variant or expression whose span comes from source rather than from a macro.
fn covers_written_code(tcx: TyCtxt<'_>, hir_id: HirId) -> bool {
    let mut finder = WrittenCode { tcx };
    let found = match tcx.hir_node(hir_id) {
        Node::Item(item) => finder.visit_item(item),
        Node::TraitItem(item) => finder.visit_trait_item(item),
        Node::ImplItem(item) => finder.visit_impl_item(item),
        Node::ForeignItem(item) => finder.visit_foreign_item(item),
        Node::Field(field) => finder.visit_field_def(field),
        Node::Variant(variant) => finder.visit_variant(variant),
        Node::Expr(expr) => finder.visit_expr(expr),
        Node::Stmt(stmt) => finder.visit_stmt(stmt),
        Node::LetStmt(local) => finder.visit_local(local),
        Node::Arm(arm) => finder.visit_arm(arm),
        Node::Param(param) => finder.visit_param(param),
        _ => {
            if is_written(tcx.hir_span(hir_id)) {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }
    };
    found.is_break()
}

fn is_written(span: Span) -> bool {
    !span.is_dummy() && !span.from_expansion()
}

fn found_or(span: Span, walk: impl FnOnce() -> ControlFlow<()>) -> ControlFlow<()> {
    if is_written(span) {
        ControlFlow::Break(())
    } else {
        walk()
    }
}

struct WrittenCode<'tcx> {
    tcx: TyCtxt<'tcx>,
}

impl<'tcx> Visitor<'tcx> for WrittenCode<'tcx> {
    type NestedFilter = nested_filter::All;
    type Result = ControlFlow<()>;

    fn maybe_tcx(&mut self) -> TyCtxt<'tcx> {
        self.tcx
    }

    fn visit_item(&mut self, item: &'tcx Item<'tcx>) -> Self::Result {
        found_or(item.span, || intravisit::walk_item(self, item))
    }

    fn visit_trait_item(&mut self, item: &'tcx TraitItem<'tcx>) -> Self::Result {
        found_or(item.span, || intravisit::walk_trait_item(self, item))
    }

    fn visit_impl_item(&mut self, item: &'tcx ImplItem<'tcx>) -> Self::Result {
        found_or(item.span, || intravisit::walk_impl_item(self, item))
    }

    fn visit_foreign_item(&mut self, item: &'tcx ForeignItem<'tcx>) -> Self::Result {
        found_or(item.span, || intravisit::walk_foreign_item(self, item))
    }

    fn visit_field_def(&mut self, field: &'tcx FieldDef<'tcx>) -> Self::Result {
        found_or(field.span, || intravisit::walk_field_def(self, field))
    }

    fn visit_variant(&mut self, variant: &'tcx Variant<'tcx>) -> Self::Result {
        found_or(variant.span, || intravisit::walk_variant(self, variant))
    }

    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) -> Self::Result {
        found_or(expr.span, || intravisit::walk_expr(self, expr))
    }
}

fn report(cx: &LateContext<'_>, hir_id: HirId, attr: &Attribute, lints: &[String]) {
    let level = attr
        .name()
        .map_or_else(String::new, |name| name.to_string());
    let lints = lints.join(", ");
    // Emitted at the parent so a generated attribute cannot silence the report about itself.
    let parent = cx.tcx.parent_hir_id(hir_id);
    span_lint_hir_and_then(
        cx,
        RQACI_SUPPRESSION_GENERATED,
        parent,
        attr.span().source_callsite(),
        format!("macro-generated `{level}({lints})` silences code written here"),
        |diag| {
            diag.note(
                "the attribute exists only after macro expansion, so the source suppression scan cannot see or record it",
            );
            diag.help(
                "fix the code so the lint passes, or make the macro leave lint levels to the caller",
            );
        },
    );
}
