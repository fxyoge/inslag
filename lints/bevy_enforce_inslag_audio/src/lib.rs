#![feature(rustc_private)]

extern crate rustc_ast;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

use std::{
    collections::HashSet,
    path::Path,
    sync::{Arc, Mutex},
};

use rustc_errors::DiagDecorator;
use rustc_hir::{
    Expr, ExprKind, HirId, Item, ItemKind,
    def::{DefKind, Res},
    def_id::DefId,
};
use rustc_lint::{EarlyContext, EarlyLintPass, LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, Ty};
use rustc_span::{Span, sym};

rustc_session::declare_lint! {
    /// Authored audio owns source selection and playback. Only the shared audio
    /// implementation and test scopes may access raw playback capabilities.
    pub BEVY_ENFORCE_INSLAG_AUDIO,
    Deny,
    "raw audio playback bypasses inslag_audio"
}

struct EnforceEnhancedAudio {
    reported: HashSet<Span>,
    test_scopes: Arc<Mutex<Vec<Span>>>,
}

struct TestScopes(Arc<Mutex<Vec<Span>>>);
rustc_session::impl_lint_pass!(TestScopes => []);
rustc_session::impl_lint_pass!(EnforceEnhancedAudio => [BEVY_ENFORCE_INSLAG_AUDIO]);
dylint_linting::dylint_library!();

impl EarlyLintPass for TestScopes {
    fn check_item(&mut self, _: &EarlyContext<'_>, item: &rustc_ast::Item) {
        if item.attrs.iter().any(|attr| {
            attr.has_name(sym::test)
                || (attr.has_name(sym::cfg)
                    && attr
                        .meta_item_list()
                        .is_some_and(|items| items.len() == 1 && items[0].has_name(sym::test)))
        }) {
            self.0.lock().unwrap().push(item.span);
        }
    }
}

#[unsafe(no_mangle)]
pub fn register_lints(_: &rustc_session::Session, store: &mut rustc_lint::LintStore) {
    let scopes = Arc::new(Mutex::new(Vec::new()));
    let early_scopes = Arc::clone(&scopes);
    store.register_pre_expansion_lint_pass(Box::new(move || {
        Box::new(TestScopes(Arc::clone(&early_scopes)))
    }));
    store.register_lints(&[BEVY_ENFORCE_INSLAG_AUDIO]);
    store.register_late_lint_pass(Box::new(move |_| {
        Box::new(EnforceEnhancedAudio {
            reported: HashSet::new(),
            test_scopes: Arc::clone(&scopes),
        })
    }));
}

impl<'tcx> LateLintPass<'tcx> for EnforceEnhancedAudio {
    fn check_path(&mut self, cx: &LateContext<'tcx>, path: &rustc_hir::Path<'tcx>, id: HirId) {
        if let Res::Def(_, definition) = path.res
            && let Some(api) = forbidden_definition(cx, definition)
        {
            self.report(cx, api, path.span, id);
        }
    }

    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if let ItemKind::Use(path, rustc_hir::UseKind::Single(_)) = item.kind {
            for res in path.res.iter().flatten() {
                if let Res::Def(_, definition) = res
                    && let Some(api) = forbidden_definition(cx, *definition)
                {
                    self.report(cx, api, path.span, item.hir_id());
                }
            }
        }
    }

    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if !matches!(expr.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) {
            return;
        }
        if let Some(method) = cx.typeck_results().type_dependent_def_id(expr.hir_id)
            && let Some(api) = forbidden_definition(cx, method)
        {
            self.report(cx, api, expr.span, expr.hir_id);
            return;
        }
        // Inferred AssetServer::load calls need not spell AudioSource anywhere.
        if let Some(api) = forbidden_type(cx, cx.typeck_results().expr_ty(expr)) {
            self.report(cx, api, expr.span, expr.hir_id);
        }
    }
}

impl EnforceEnhancedAudio {
    fn report(&mut self, cx: &LateContext<'_>, api: DefId, span: Span, id: HirId) {
        let span = span.source_callsite();
        if cx.tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).as_str() == "inslag_audio"
            || is_test_scope(cx, id, span, &self.test_scopes.lock().unwrap())
            || self.reported.iter().any(|reported| reported.contains(span))
        {
            return;
        }
        self.reported.insert(span);
        cx.emit_span_lint(BEVY_ENFORCE_INSLAG_AUDIO, span, DiagDecorator(|diag| {
            diag.primary_message(format!("raw audio API `{}` bypasses authored audio playback", cx.tcx.item_name(api)));
            diag.help("select asset references through `inslag_audio` SCXML effects or activities; use `AudioMixer` for user volume preferences");
        }));
    }
}

fn forbidden_definition(cx: &LateContext<'_>, id: DefId) -> Option<DefId> {
    if canonical_forbidden(cx, id) {
        return Some(id);
    }
    match cx.tcx.def_kind(id) {
        DefKind::TyAlias => forbidden_type(
            cx,
            cx.tcx.type_of(id).instantiate_identity().skip_norm_wip(),
        ),
        DefKind::Ctor(..) | DefKind::Variant => forbidden_definition(cx, cx.tcx.parent(id)),
        DefKind::AssocFn | DefKind::AssocConst { .. } | DefKind::AssocTy => {
            if let Some(implementation) = cx.tcx.impl_of_assoc(id) {
                forbidden_type(
                    cx,
                    cx.tcx
                        .type_of(implementation)
                        .instantiate_identity()
                        .skip_norm_wip(),
                )
            } else {
                forbidden_definition(cx, cx.tcx.parent(id))
            }
        }
        _ => None,
    }
}

fn forbidden_type(cx: &LateContext<'_>, ty: Ty<'_>) -> Option<DefId> {
    ty.walk()
        .filter_map(|argument| argument.as_type())
        .find_map(|ty| match ty.kind() {
            ty::Adt(definition, _) if canonical_forbidden(cx, definition.did()) => {
                Some(definition.did())
            }
            _ => None,
        })
}

fn canonical_forbidden(cx: &LateContext<'_>, id: DefId) -> bool {
    let name = cx.tcx.item_name(id);
    let forbidden = match cx.tcx.crate_name(id.krate).as_str() {
        "bevy_audio" => matches!(
            name.as_str(),
            "AudioSource"
                | "AudioLoader"
                | "AudioPlayer"
                | "AudioSink"
                | "SpatialAudioSink"
                | "AudioSinkPlayback"
                | "Decodable"
                | "AddAudioSource"
                | "Pitch"
        ),
        "rodio" => matches!(
            name.as_str(),
            "Player" | "SpatialPlayer" | "MixerDeviceSink" | "DeviceSinkBuilder"
        ),
        _ => false,
    };
    if !forbidden {
        return false;
    }
    let root = DefId {
        krate: id.krate,
        index: rustc_hir::def_id::CRATE_DEF_INDEX,
    };
    cx.tcx
        .module_children(root)
        .iter()
        .any(|child| child.ident.name == name && child.res.opt_def_id() == Some(id))
}

fn is_test_scope(cx: &LateContext<'_>, id: HirId, span: Span, scopes: &[Span]) -> bool {
    let filename = cx
        .sess()
        .source_map()
        .lookup_char_pos(span.lo())
        .file
        .name
        .prefer_local_unconditionally()
        .to_string();
    let path = Path::new(&filename);
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "tests.rs" || name.ends_with("_tests.rs"))
        || path.components().any(|part| part.as_os_str() == "tests")
    {
        return true;
    }
    std::iter::once(id)
        .chain(cx.tcx.hir_parent_iter(id).map(|(id, _)| id))
        .any(|id| {
            scopes
                .iter()
                .any(|scope| scope.contains(cx.tcx.hir_span(id)))
        })
}

#[cfg(test)]
mod tests {
    #[test]
    fn trusted_boundary() {
        dylint_testing::ui::Test::example(env!("CARGO_PKG_NAME"), "trusted_boundary")
            .rustc_flags(["--crate-name", "inslag_audio"])
            .run();
    }
    #[test]
    fn allowed() {
        dylint_testing::ui_test_example(env!("CARGO_PKG_NAME"), "allowed");
    }
    #[test]
    fn forbidden() {
        dylint_testing::ui_test_example(env!("CARGO_PKG_NAME"), "forbidden");
    }
    #[test]
    fn macros() {
        dylint_testing::ui_test_example(env!("CARGO_PKG_NAME"), "macros");
    }
    #[test]
    fn test_scopes() {
        dylint_testing::ui::Test::example(env!("CARGO_PKG_NAME"), "test_scopes")
            .rustc_flags(["--test"])
            .run();
    }
}
