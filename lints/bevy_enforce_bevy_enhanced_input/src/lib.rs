#![feature(rustc_private)]

extern crate rustc_ast;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

use rustc_errors::DiagDecorator;
use rustc_hir::{HirId, Item, ItemKind, def::Res, def_id::DefId};
use rustc_lint::{EarlyContext, EarlyLintPass, LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, Ty};
use rustc_span::{Span, sym};

rustc_session::declare_lint! {
    /// Production input must go through bevy_enhanced_input. Test scopes may
    /// inject raw input; intentional production exceptions can allow this lint.
    pub BEVY_ENFORCE_BEVY_ENHANCED_INPUT,
    Warn,
    "raw Bevy input bypasses bevy_enhanced_input"
}

struct EnforceEnhancedInput {
    reported: HashSet<(Span, DefId)>,
    test_scopes: Arc<Mutex<Vec<Span>>>,
}

struct TestScopes(Arc<Mutex<Vec<Span>>>);
rustc_session::impl_lint_pass!(TestScopes => []);

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

rustc_session::impl_lint_pass!(EnforceEnhancedInput => [BEVY_ENFORCE_BEVY_ENHANCED_INPUT]);
dylint_linting::dylint_library!();

#[unsafe(no_mangle)]
pub fn register_lints(_: &rustc_session::Session, store: &mut rustc_lint::LintStore) {
    // cfg and test attributes are consumed before HIR lowering. Retain only
    // their source scopes before expansion; all API matching stays resolved.
    let scopes = Arc::new(Mutex::new(Vec::new()));
    let early_scopes = Arc::clone(&scopes);
    store.register_pre_expansion_lint_pass(Box::new(move || {
        Box::new(TestScopes(Arc::clone(&early_scopes)))
    }));
    store.register_lints(&[BEVY_ENFORCE_BEVY_ENHANCED_INPUT]);
    store.register_late_lint_pass(Box::new(move |_| {
        Box::new(EnforceEnhancedInput {
            reported: HashSet::new(),
            test_scopes: Arc::clone(&scopes),
        })
    }));
}

impl<'tcx> LateLintPass<'tcx> for EnforceEnhancedInput {
    fn check_path(&mut self, cx: &LateContext<'tcx>, path: &rustc_hir::Path<'tcx>, id: HirId) {
        if let Res::Def(_, def_id) = path.res {
            self.check_definition(cx, def_id, path.span, id);
        }
    }

    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        // Use paths have a separate, per-namespace representation in HIR.
        if let ItemKind::Use(path, rustc_hir::UseKind::Single(_)) = item.kind {
            for res in path.res.iter().flatten() {
                if let Res::Def(_, def_id) = res {
                    self.check_definition(cx, *def_id, path.span, item.hir_id());
                }
            }
        }
    }
}

impl EnforceEnhancedInput {
    fn check_definition(&mut self, cx: &LateContext<'_>, def_id: DefId, span: Span, id: HirId) {
        // Check caller-written macro arguments, but not framework-generated
        // input plumbing or generated test harness code.
        if span.from_expansion() || is_test_scope(cx, id, span, &self.test_scopes.lock().unwrap()) {
            return;
        }
        let mut forbidden = Vec::new();
        if is_forbidden(cx, def_id) {
            forbidden.push(def_id);
        } else if matches!(cx.tcx.def_kind(def_id), rustc_hir::def::DefKind::TyAlias) {
            collect_forbidden(
                cx,
                cx.tcx
                    .type_of(def_id)
                    .instantiate_identity()
                    .skip_norm_wip(),
                &mut forbidden,
            );
        } else if matches!(
            cx.tcx.def_kind(def_id),
            rustc_hir::def::DefKind::Ctor(..) | rustc_hir::def::DefKind::Variant
        ) {
            let mut parent = cx.tcx.parent(def_id);
            if matches!(cx.tcx.def_kind(parent), rustc_hir::def::DefKind::Variant) {
                parent = cx.tcx.parent(parent);
            }
            if is_forbidden(cx, parent) {
                forbidden.push(parent);
            }
        }
        for api in forbidden {
            if !self.reported.insert((span, api)) {
                continue;
            }
            cx.emit_span_lint(
                BEVY_ENFORCE_BEVY_ENHANCED_INPUT,
                span,
                DiagDecorator(|diag| {
                    diag.primary_message(format!(
                        "raw Bevy input API `{}` is forbidden in production source",
                        cx.tcx.item_name(api),
                    ));
                    diag.help("use `bevy_enhanced_input` instead");
                }),
            );
        }
    }
}

fn is_forbidden(cx: &LateContext<'_>, id: DefId) -> bool {
    if cx.tcx.crate_name(id.krate).as_str() != "bevy_input" {
        return false;
    }
    let name = cx.tcx.item_name(id);
    let module = match name.as_str() {
        "ButtonInput" => None,
        "KeyboardInput" => Some("keyboard"),
        "MouseButtonInput"
        | "MouseMotion"
        | "MouseWheel"
        | "AccumulatedMouseMotion"
        | "AccumulatedMouseScroll" => Some("mouse"),
        "Gamepad"
        | "GamepadEvent"
        | "RawGamepadEvent"
        | "GamepadButtonStateChangedEvent"
        | "GamepadButtonChangedEvent"
        | "GamepadAxisChangedEvent"
        | "RawGamepadButtonChangedEvent"
        | "RawGamepadAxisChangedEvent" => Some("gamepad"),
        _ => return false,
    };
    let mut parent = DefId {
        krate: id.krate,
        index: rustc_hir::def_id::CRATE_DEF_INDEX,
    };
    if let Some(module) = module {
        let Some(module_id) = cx
            .tcx
            .module_children(parent)
            .iter()
            .find(|child| child.ident.name.as_str() == module)
            .and_then(|child| child.res.opt_def_id())
        else {
            return false;
        };
        parent = module_id;
    }
    // Compare definition identities, including the root re-export of
    // ButtonInput. Diagnostic path strings may use a consumer's re-exports.
    cx.tcx
        .module_children(parent)
        .iter()
        .any(|child| child.ident.name == name && child.res.opt_def_id() == Some(id))
}

fn collect_forbidden(cx: &LateContext<'_>, ty: Ty<'_>, found: &mut Vec<DefId>) {
    for arg in ty.walk() {
        if let Some(ty) = arg.as_type()
            && let ty::Adt(def, _) = ty.kind()
            && is_forbidden(cx, def.did())
        {
            found.push(def.did());
        }
    }
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
            if scopes
                .iter()
                .any(|scope| scope.contains(cx.tcx.hir_span(id)))
            {
                return true;
            }
            false
        })
}

#[cfg(test)]
mod tests {
    macro_rules! examples {
        ($($name:ident),+ $(,)?) => {
            $(#[test]
            fn $name() {
                dylint_testing::ui_test_example(env!("CARGO_PKG_NAME"), stringify!($name));
            })+
        };
    }

    examples!(allowed, forbidden, resolved_paths, macros, test_files);

    #[test]
    fn test_scopes() {
        dylint_testing::ui::Test::example(env!("CARGO_PKG_NAME"), "test_scopes")
            .rustc_flags(["--test"])
            .run();
    }
}
