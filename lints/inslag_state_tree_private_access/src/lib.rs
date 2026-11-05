#![feature(rustc_private)]

extern crate rustc_ast;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

use std::path::Path;
use std::sync::{Arc, Mutex};

use rustc_errors::DiagDecorator;
use rustc_hir::{Expr, ExprKind, HirId, def::Res, def_id::DefId};
use rustc_lint::{EarlyContext, EarlyLintPass, LateContext, LateLintPass, LintContext};
use rustc_middle::ty;
use rustc_span::{Span, sym};

rustc_session::declare_lint! {
    /// Production Rust should interact with statecharts through events, effects,
    /// and activities, not private datamodel values or authored states. Tests
    /// may inspect internals; intentional integration adapters can allow the lint.
    pub INSLAG_STATE_TREE_PRIVATE_ACCESS,
    Warn,
    "production Rust inspects private statechart internals"
}

struct PrivateAccess {
    test_scopes: Arc<Mutex<Vec<Span>>>,
}

struct TestScopes(Arc<Mutex<Vec<Span>>>);
rustc_session::impl_lint_pass!(TestScopes => []);
rustc_session::impl_lint_pass!(PrivateAccess => [INSLAG_STATE_TREE_PRIVATE_ACCESS]);
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
    // cfg/test attributes are consumed before HIR lowering. Retain their source
    // scopes before expansion, while matching APIs only after type checking.
    let scopes = Arc::new(Mutex::new(Vec::new()));
    let early_scopes = Arc::clone(&scopes);
    store.register_pre_expansion_lint_pass(Box::new(move || {
        Box::new(TestScopes(Arc::clone(&early_scopes)))
    }));
    store.register_lints(&[INSLAG_STATE_TREE_PRIVATE_ACCESS]);
    store.register_late_lint_pass(Box::new(move |_| {
        Box::new(PrivateAccess {
            test_scopes: Arc::clone(&scopes),
        })
    }));
}

impl<'tcx> LateLintPass<'tcx> for PrivateAccess {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let (id, span) = match expr.kind {
            ExprKind::MethodCall(segment, ..) => {
                let Some(id) = cx.typeck_results().type_dependent_def_id(expr.hir_id) else {
                    return;
                };
                (id, segment.ident.span)
            }
            // Checking the method path also catches UFCS calls and accessors
            // captured as function values, without reporting the call twice.
            ExprKind::Path(ref path) => {
                let Res::Def(_, id) = cx.qpath_res(path, expr.hir_id) else {
                    return;
                };
                (id, expr.span)
            }
            _ => return,
        };
        let Some(access) = private_access(cx, id) else {
            return;
        };
        // Check caller-written macro arguments, not generated framework code.
        if span.from_expansion()
            || is_test_scope(cx, expr.hir_id, span, &self.test_scopes.lock().unwrap())
        {
            return;
        }
        cx.emit_span_lint(
            INSLAG_STATE_TREE_PRIVATE_ACCESS,
            span,
            DiagDecorator(|diag| {
                diag.primary_message(match access {
                    Access::Data | Access::ContextData => {
                        "reading state-tree datamodel data couples Rust code to private statechart internals"
                    }
                    Access::States => {
                        "inspecting authored state-tree states couples Rust code to private statechart internals"
                    }
                });
                diag.help(match access {
                    Access::ContextData => {
                        "pass explicitly authored effect/activity arguments through `StateTreeArgs` instead"
                    }
                    Access::Data | Access::States => {
                        "send an event and let the statechart decide whether to handle it, or expose the needed information through an effect or activity"
                    }
                });
            }),
        );
    }
}

enum Access {
    Data,
    States,
    ContextData,
}

fn private_access(cx: &LateContext<'_>, id: DefId) -> Option<Access> {
    if cx.tcx.crate_name(id.krate).as_str() != "inslag_state_tree" {
        return None;
    }
    let method = cx.tcx.item_name(id);
    if !matches!(
        method.as_str(),
        "data" | "active_states" | "active_state_names"
    ) {
        return None;
    }
    let impl_id = cx.tcx.impl_of_assoc(id)?;
    let self_ty = cx
        .tcx
        .type_of(impl_id)
        .instantiate_identity()
        .skip_norm_wip();
    let ty::Adt(owner, _) = self_ty.kind() else {
        return None;
    };
    let root = DefId {
        krate: id.krate,
        index: rustc_hir::def_id::CRATE_DEF_INDEX,
    };
    // Match canonical exported type identities, not diagnostic path strings or
    // caller spelling. Context aliases all resolve to StateTreeContext's impl.
    let owner_name = cx.tcx.item_name(owner.did());
    let children = if let Some(root) = root.as_local() {
        cx.tcx.module_children_local(root)
    } else {
        cx.tcx.module_children(root)
    };
    if !children
        .iter()
        .any(|child| child.ident.name == owner_name && child.res.opt_def_id() == Some(owner.did()))
    {
        return None;
    }
    match (owner_name.as_str(), method.as_str()) {
        ("StateTreeInstance", "data") => Some(Access::Data),
        ("StateTreeInstance", "active_states" | "active_state_names") => Some(Access::States),
        ("StateTreeContext", "data") => Some(Access::ContextData),
        _ => None,
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
            scopes
                .iter()
                .any(|scope| scope.contains(cx.tcx.hir_span(id)))
        })
}

#[cfg(test)]
mod tests {
    macro_rules! examples {
        ($($name:ident),+ $(,)?) => {
            $(#[test]
            fn $name() {
                dylint_testing::ui_test_example(
                    env!("CARGO_PKG_NAME"),
                    concat!("state_tree_", stringify!($name)),
                );
            })+
        };
    }

    examples!(allowed, forbidden, resolved_paths, macros, test_files);

    #[test]
    fn local_definitions() {
        dylint_testing::ui::Test::example(env!("CARGO_PKG_NAME"), "state_tree_local_definitions")
            .rustc_flags(["--crate-name=inslag_state_tree"])
            .run();
    }

    #[test]
    fn test_scopes() {
        dylint_testing::ui::Test::example(env!("CARGO_PKG_NAME"), "state_tree_test_scopes")
            .rustc_flags(["--test"])
            .run();
    }
}
