#![feature(default_field_values, rustc_private)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;

use std::ops::ControlFlow;

use rustc_errors::DiagDecorator;
use rustc_hir::{
    Expr, ExprKind, Mutability,
    def::DefKind,
    def_id::DefId,
    intravisit::{self, Visitor},
};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, Ty, TypeckResults};

rustc_lint::declare_lint! {
    /// Keep system registration visible in Bevy Plugin::build.
    pub BEVY_INDIRECT_SYSTEM_REGISTRATION,
    Warn,
    "helper calls in Plugin::build hide system registration"
}

rustc_lint::declare_lint_pass!(BevyIndirectSystemRegistration => [BEVY_INDIRECT_SYSTEM_REGISTRATION]);

dylint_linting::dylint_library!();

#[unsafe(no_mangle)]
pub fn register_lints(_: &rustc_session::Session, store: &mut rustc_lint::LintStore) {
    store.register_lints(&[BEVY_INDIRECT_SYSTEM_REGISTRATION]);
    store.register_late_lint_pass(Box::new(|_| Box::new(BevyIndirectSystemRegistration)));
}

impl<'tcx> LateLintPass<'tcx> for BevyIndirectSystemRegistration {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion()
            || !matches!(expr.kind, ExprKind::Call(..) | ExprKind::MethodCall(..))
        {
            return;
        }

        let owner = cx.tcx.hir_enclosing_body_owner(expr.hir_id);
        let Some(trait_item) = cx
            .tcx
            .opt_associated_item(owner.to_def_id())
            .and_then(|item| item.trait_item_def_id())
        else {
            return;
        };
        if cx.tcx.item_name(trait_item).as_str() != "build"
            || definition(cx, trait_item.krate, "bevy_app", &["Plugin"])
                != Some(cx.tcx.parent(trait_item))
        {
            return;
        }

        let typeck = cx.typeck_results();
        let callee = called_function(typeck, expr);
        // Function pointers can hide configuration production without a known body.
        if callee.is_none()
            && !matches!(expr.kind, ExprKind::Call(target, _) if matches!(typeck.expr_ty(target).kind(), ty::FnPtr(..)))
        {
            return;
        }
        // Bevy's configuration combinators (chain, in_set, run_if, etc.) are
        // composition at the registration site, not user-authored factories.
        if callee.is_some_and(|id| cx.tcx.crate_name(id.krate).as_str() == "bevy_ecs") {
            return;
        }

        let (message, help) = if is_system_configuration(cx, typeck.expr_ty(expr)) {
            (
                "calling a system configuration helper in Plugin::build hides system registration",
                "move the system tuple and its configuration into the registration site",
            )
        } else if callee.is_some_and(|id| registers_app_systems(cx, id)) {
            (
                "calling a system registration helper in Plugin::build hides system registration",
                "move the `add_systems` calls into `Plugin::build`",
            )
        } else {
            return;
        };
        cx.emit_span_lint(
            BEVY_INDIRECT_SYSTEM_REGISTRATION,
            expr.span,
            DiagDecorator(|diag| {
                diag.primary_message(message);
                diag.help(help);
            }),
        );
    }
}

fn called_function(typeck: &TypeckResults<'_>, expr: &Expr<'_>) -> Option<DefId> {
    match expr.kind {
        ExprKind::Call(target, _) => match typeck.expr_ty(target).kind() {
            ty::FnDef(id, _) => Some(*id),
            _ => None,
        },
        ExprKind::MethodCall(..) => typeck.type_dependent_def_id(expr.hir_id),
        _ => None,
    }
}

fn is_app(cx: &LateContext<'_>, ty: Ty<'_>) -> bool {
    matches!(ty.kind(), ty::Adt(adt, _)
        if definition(cx, adt.did().krate, "bevy_app", &["App"]) == Some(adt.did()))
}

fn registers_app_systems(cx: &LateContext<'_>, id: DefId) -> bool {
    let Some(local) = id.as_local() else {
        return false;
    };
    // A trait default body may not be the implementation the call invokes.
    if cx.tcx.def_kind(cx.tcx.parent(id)) == DefKind::Trait {
        return false;
    }
    let Some(body) = cx.tcx.hir_maybe_body_owned_by(local) else {
        return false;
    };
    let typeck = cx.tcx.typeck(local);
    body.params.iter().any(|param| {
        matches!(typeck.node_type(param.pat.hir_id).kind(),
            ty::Ref(_, inner, Mutability::Mut) if is_app(cx, *inner))
    }) && FindAppRegistration { cx, typeck }
        .visit_expr(body.value)
        .is_break()
}

struct FindAppRegistration<'a, 'tcx> {
    cx: &'a LateContext<'tcx>,
    typeck: &'tcx TypeckResults<'tcx>,
}

impl<'tcx> Visitor<'tcx> for FindAppRegistration<'_, 'tcx> {
    type Result = ControlFlow<()>;

    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) -> Self::Result {
        let cx = self.cx;
        if called_function(self.typeck, expr).is_some_and(|id| {
            cx.tcx.crate_name(id.krate).as_str() == "bevy_app"
                && cx.tcx.item_name(id).as_str() == "add_systems"
                && cx.tcx.impl_of_assoc(id).is_some_and(|implementation| {
                    is_app(
                        cx,
                        cx.tcx
                            .type_of(implementation)
                            .instantiate_identity()
                            .skip_norm_wip(),
                    )
                })
        }) {
            return ControlFlow::Break(());
        }
        intravisit::walk_expr(self, expr)
    }
}

fn definition(
    cx: &LateContext<'_>,
    krate: rustc_hir::def_id::CrateNum,
    crate_name: &str,
    path: &[&str],
) -> Option<DefId> {
    if cx.tcx.crate_name(krate).as_str() != crate_name {
        return None;
    }
    let mut current = DefId {
        krate,
        index: rustc_hir::def_id::CRATE_DEF_INDEX,
    };
    for name in path {
        current = cx
            .tcx
            .module_children(current)
            .iter()
            .find(|item| item.ident.name.as_str() == *name)?
            .res
            .opt_def_id()?;
    }
    Some(current)
}

fn is_bevy_type(cx: &LateContext<'_>, def_id: DefId, path: &[&str]) -> bool {
    definition(cx, def_id.krate, "bevy_ecs", path) == Some(def_id)
}

fn is_schedule_system<'tcx>(cx: &LateContext<'tcx>, ty: Ty<'tcx>) -> bool {
    // ScheduleSystem is an alias for Box<dyn System<In = (), Out = ()>>,
    // so compare the resolved type rather than looking for an ADT named ScheduleSystem.
    cx.tcx
        .crates(())
        .iter()
        .filter(|&&krate| cx.tcx.crate_name(krate).as_str() == "bevy_ecs")
        .any(|&krate| {
            let Some(alias) = definition(cx, krate, "bevy_ecs", &["system", "ScheduleSystem"])
            else {
                return false;
            };
            cx.tcx.erase_and_anonymize_regions(ty)
                == cx.tcx.erase_and_anonymize_regions(
                    cx.tcx.type_of(alias).instantiate_identity().skip_norm_wip(),
                )
        })
}

fn is_system_configuration<'tcx>(cx: &LateContext<'tcx>, output: Ty<'tcx>) -> bool {
    match output.kind() {
        ty::Adt(def, args) => {
            is_bevy_type(cx, def.did(), &["schedule", "ScheduleConfigs"])
                && args
                    .types()
                    .next()
                    .is_some_and(|ty| is_schedule_system(cx, ty))
        }
        ty::Alias(
            _,
            ty::AliasTy {
                kind: ty::Opaque { def_id },
                ..
            },
        ) => cx
            .tcx
            .explicit_item_bounds(*def_id)
            .iter_identity_copied()
            .any(|bound| {
                let (clause, _) = bound.skip_norm_wip();
                clause.as_trait_clause().is_some_and(|bound| {
                    let trait_ref = bound.skip_binder().trait_ref;
                    is_bevy_type(cx, trait_ref.def_id, &["schedule", "IntoScheduleConfigs"])
                        && trait_ref
                            .args
                            .types()
                            .nth(1)
                            .is_some_and(|ty| is_schedule_system(cx, ty))
                })
            }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    macro_rules! examples {
        ($($name:ident),+ $(,)?) => {
            $(
                #[test]
                fn $name() {
                    dylint_testing::ui_test_example(env!("CARGO_PKG_NAME"), stringify!($name));
                }
            )+
        };
    }

    examples!(
        concrete_factory,
        aliased_factory,
        opaque_factory,
        generic_opaque_factory,
        method_factory,
        nested_factory,
        local_factory,
        function_pointer_factory,
        app_setup,
        outside_plugin,
        inline_configuration,
        system_set_factory,
        unrelated_build,
        unrelated_configuration,
        nested_bodies,
        allowed_call,
    );
}
