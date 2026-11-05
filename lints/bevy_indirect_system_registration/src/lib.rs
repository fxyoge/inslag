#![feature(rustc_private)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;

use rustc_errors::DiagDecorator;
use rustc_hir::{Expr, ExprKind, def_id::DefId};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, Ty};

rustc_session::declare_lint! {
    /// Rejects configuration-producing helper calls inside Bevy Plugin::build.
    /// Keep system composition visible at its registration site instead.
    /// App-mutating setup helpers and system-set configurations are outside this rule.
    pub BEVY_INDIRECT_SYSTEM_REGISTRATION,
    Warn,
    "configuration-producing helper calls in Plugin::build hide system registration"
}

rustc_session::declare_lint_pass!(BevyIndirectSystemRegistration => [BEVY_INDIRECT_SYSTEM_REGISTRATION]);

dylint_linting::dylint_library!();

#[unsafe(no_mangle)]
pub fn register_lints(_: &rustc_session::Session, store: &mut rustc_lint::LintStore) {
    store.register_lints(&[BEVY_INDIRECT_SYSTEM_REGISTRATION]);
    store.register_late_lint_pass(Box::new(|_| Box::new(BevyIndirectSystemRegistration)));
}

impl<'tcx> LateLintPass<'tcx> for BevyIndirectSystemRegistration {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion() {
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

        let callee = match expr.kind {
            ExprKind::Call(callee, _) => match cx.typeck_results().expr_ty(callee).kind() {
                ty::FnDef(def_id, _) => Some(*def_id),
                // Function pointers still hide configuration production, even without
                // a statically known definition.
                ty::FnPtr(..) => None,
                _ => return,
            },
            ExprKind::MethodCall(..) => cx.typeck_results().type_dependent_def_id(expr.hir_id),
            _ => return,
        };
        // Bevy's configuration combinators (chain, in_set, run_if, etc.) are
        // composition at the registration site, not user-authored factories.
        if callee.is_some_and(|id| cx.tcx.crate_name(id.krate).as_str() == "bevy_ecs") {
            return;
        }

        let output = cx.typeck_results().expr_ty(expr);
        if is_system_configuration(cx, output) {
            cx.emit_span_lint(
                BEVY_INDIRECT_SYSTEM_REGISTRATION,
                expr.span,
                DiagDecorator(|diag| {
                    diag.primary_message(
                        "calling a system configuration helper in Plugin::build hides system registration",
                    );
                    diag.help(
                        "move the system tuple and its configuration into the registration site",
                    );
                }),
            );
        }
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
        outside_plugin,
        inline_configuration,
        system_set_factory,
        unrelated_build,
        unrelated_configuration,
        nested_bodies,
        allowed_call,
    );
}
