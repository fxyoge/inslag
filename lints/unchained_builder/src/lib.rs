#![feature(rustc_private)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

use rustc_errors::DiagDecorator;
use rustc_hir::{
    BindingMode, Block, Expr, ExprKind, HirId, Mutability, PatKind, StmtKind,
    def::Res,
    intravisit::{self, Visitor},
};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::TypeVisitable;
use rustc_middle::ty::adjustment::{Adjust, DerefAdjustKind};
use rustc_middle::ty::{self, Ty, TypeckResults};
use rustc_span::Span;

mod receiver;
use receiver::ReceiverIdentity;

rustc_session::declare_lint! {
    pub UNCHAINED_BUILDER,
    Warn,
    "builder calls should be chained"
}

#[derive(Default)]
struct UnchainedBuilder<'tcx> {
    identity: ReceiverIdentity<'tcx>,
}
rustc_session::impl_lint_pass!(UnchainedBuilder<'_> => [UNCHAINED_BUILDER]);
dylint_linting::dylint_library!();

#[unsafe(no_mangle)]
pub fn register_lints(_: &rustc_session::Session, store: &mut rustc_lint::LintStore) {
    store.register_lints(&[UNCHAINED_BUILDER]);
    store.register_late_lint_pass(Box::new(|_| Box::new(UnchainedBuilder::default())));
}

impl<'tcx> LateLintPass<'tcx> for UnchainedBuilder<'tcx> {
    fn check_block(&mut self, cx: &LateContext<'tcx>, block: &'tcx Block<'tcx>) {
        let mut run: Option<Run<'tcx>> = None;
        for statement in block
            .stmts
            .iter()
            .map(|stmt| stmt.kind)
            .chain(block.expr.map(StmtKind::Expr))
        {
            let operation = operation(cx, &mut self.identity, statement);
            if let (Some(previous), Some(next)) = (&mut run, &operation)
                && previous.last.output.is_some()
                && previous.last.output == next.input
                && previous.last.style == next.style
                && same_type(cx, previous.last.output_ty, next.input_ty)
                && (!previous.last.binding
                    || single_use(cx, previous.last.output.unwrap(), next.receiver))
            {
                previous.last = *next;
                previous.count += 1;
                continue;
            }
            if let Some(previous) = run.take() {
                emit(cx, previous);
            }
            run = operation.map(|first| Run {
                first,
                last: first,
                count: 1,
            });
        }
        if let Some(run) = run {
            emit(cx, run);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Mutable,
    Consuming,
}

#[derive(Clone, Copy)]
struct Operation<'tcx> {
    input: Option<HirId>,
    output: Option<HirId>,
    input_ty: Ty<'tcx>,
    output_ty: Ty<'tcx>,
    receiver: HirId,
    binding: bool,
    style: Style,
    hir_id: HirId,
    span: Span,
}

struct Run<'tcx> {
    first: Operation<'tcx>,
    last: Operation<'tcx>,
    count: usize,
}

fn emit(cx: &LateContext<'_>, run: Run<'_>) {
    if run.count < 2 {
        return;
    }
    cx.tcx.emit_node_span_lint(
        UNCHAINED_BUILDER,
        run.first.hir_id,
        run.first.span.to(run.last.span),
        DiagDecorator(|diag| {
            diag.primary_message("builder calls should be chained");
            diag.help("combine these builder calls into a single method chain");
        }),
    );
}

fn typeck<'tcx>(cx: &LateContext<'tcx>, expr: &Expr<'tcx>) -> &'tcx TypeckResults<'tcx> {
    cx.tcx.typeck(cx.tcx.hir_enclosing_body_owner(expr.hir_id))
}

fn local<'tcx>(cx: &LateContext<'tcx>, expr: &Expr<'tcx>) -> Option<HirId> {
    match expr.kind {
        ExprKind::Path(ref path) => match typeck(cx, expr).qpath_res(path, expr.hir_id) {
            Res::Local(id) => Some(id),
            _ => None,
        },
        ExprKind::AddrOf(_, _, inner) => local(cx, inner),
        ExprKind::Unary(rustc_hir::UnOp::Deref, inner)
            if matches!(typeck(cx, inner).expr_ty(inner).kind(), ty::Ref(..)) =>
        {
            local(cx, inner)
        }
        _ => None,
    }
}

fn operation<'tcx>(
    cx: &LateContext<'tcx>,
    identity: &mut ReceiverIdentity<'tcx>,
    stmt: StmtKind<'tcx>,
) -> Option<Operation<'tcx>> {
    match stmt {
        StmtKind::Semi(expr) | StmtKind::Expr(expr) => expression_operation(cx, identity, expr),
        StmtKind::Let(binding) if binding.els.is_none() && !binding.span.from_expansion() => {
            let PatKind::Binding(BindingMode::NONE | BindingMode::MUT, output, _, None) =
                binding.pat.kind
            else {
                return None;
            };
            let mut operation = chain(cx, identity, binding.init?)?;
            if operation.style != Style::Consuming {
                return None;
            }
            operation.output = Some(output);
            operation.binding = true;
            Some(operation)
        }
        _ => None,
    }
}

fn expression_operation<'tcx>(
    cx: &LateContext<'tcx>,
    identity: &mut ReceiverIdentity<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Option<Operation<'tcx>> {
    if expr.span.from_expansion() {
        return None;
    }
    if let ExprKind::Assign(left, right, _) = expr.kind {
        let output = local(cx, left)?;
        let mut operation = chain(cx, identity, right)?;
        if operation.style != Style::Consuming || operation.input != Some(output) {
            return None;
        }
        operation.output = Some(output);
        Some(operation)
    } else {
        let mut operation = chain(cx, identity, expr)?;
        operation.output = match operation.style {
            Style::Mutable => operation.input,
            Style::Consuming => None,
        };
        Some(operation)
    }
}

fn same_type<'tcx>(cx: &LateContext<'tcx>, left: Ty<'tcx>, right: Ty<'tcx>) -> bool {
    cx.tcx.erase_and_anonymize_regions(left) == cx.tcx.erase_and_anonymize_regions(right)
}

fn method<'tcx>(
    cx: &LateContext<'tcx>,
    identity: &mut ReceiverIdentity<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Option<(Style, &'tcx Expr<'tcx>, &'tcx [Expr<'tcx>])> {
    if expr.span.from_expansion() {
        return None;
    }
    let (receiver, arguments, id, generics) = match expr.kind {
        ExprKind::MethodCall(_, receiver, arguments, _) => {
            let results = typeck(cx, expr);
            (
                receiver,
                arguments,
                results.type_dependent_def_id(expr.hir_id)?,
                results.node_args(expr.hir_id),
            )
        }
        ExprKind::Call(callee, [receiver, arguments @ ..]) => {
            let ty::FnDef(id, generics) = *typeck(cx, callee).expr_ty(callee).kind() else {
                return None;
            };
            if !cx.tcx.opt_associated_item(id)?.is_method() {
                return None;
            }
            // A qualified trait call need not have an unambiguous method-call
            // equivalent in this scope.
            let implementation = cx.tcx.opt_associated_item(id)?.impl_container(cx.tcx)?;
            if cx.tcx.impl_is_of_trait(implementation) {
                return None;
            }
            (receiver, arguments, id, generics.skip_binder())
        }
        _ => return None,
    };
    let signature = cx
        .tcx
        .fn_sig(id)
        .instantiate(cx.tcx, generics)
        .skip_norm_wip()
        .skip_binder();
    if signature.safety().is_unsafe() {
        return None;
    }
    let input = *signature.inputs().first()?;
    let output = signature.output();
    let style = match (input.kind(), output.kind()) {
        (ty::Ref(_, input, Mutability::Mut), ty::Ref(_, output, Mutability::Mut))
            if same_type(cx, *input, *output) =>
        {
            if !independent_return_lifetime(cx, id) {
                return None;
            }
            let env = cx.typing_env().with_post_analysis_normalized(cx.tcx);
            let instance = ty::Instance::try_resolve(cx.tcx, env, id, generics).ok()??;
            if !identity.returns_receiver(cx.tcx, env, instance) {
                return None;
            }
            Style::Mutable
        }
        (ty::Adt(..) | ty::Param(..) | ty::Alias(..), _) if same_type(cx, input, output) => {
            Style::Consuming
        }
        _ => return None,
    };
    Some((style, receiver, arguments))
}

fn independent_return_lifetime(cx: &LateContext<'_>, id: rustc_hir::def_id::DefId) -> bool {
    use std::ops::ControlFlow;

    struct CoupledLifetime<'tcx>(ty::BoundRegion<'tcx>);
    impl<'tcx> ty::TypeVisitor<ty::TyCtxt<'tcx>> for CoupledLifetime<'tcx> {
        type Result = ControlFlow<()>;
        fn visit_region(&mut self, region: ty::Region<'tcx>) -> Self::Result {
            if matches!(region.kind(), ty::ReBound(_, bound) if bound.var == self.0.var) {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }
    }
    let signature = cx
        .tcx
        .fn_sig(id)
        .instantiate_identity()
        .skip_norm_wip()
        .skip_binder();
    let ty::Ref(region, _, _) = *signature.output().kind() else {
        return false;
    };
    let ty::ReBound(_, bound) = region.kind() else {
        return false;
    };
    if !matches!(signature.inputs()[0].kind(), ty::Ref(input, _, _) if *input == region) {
        return false;
    }
    // An early-bound return lifetime can be constrained by where-clauses. A
    // shared late-bound lifetime in another input can extend that input's borrow.
    !signature.inputs()[1..]
        .iter()
        .any(|input| input.visit_with(&mut CoupledLifetime(bound)).is_break())
}

fn chain<'tcx>(
    cx: &LateContext<'tcx>,
    identity: &mut ReceiverIdentity<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Option<Operation<'tcx>> {
    let (style, mut receiver, arguments) = method(cx, identity, expr)?;
    let mut all_arguments: Vec<_> = arguments.iter().collect();
    while let Some((inner_style, inner_receiver, arguments)) = method(cx, identity, receiver) {
        if inner_style != style {
            return None;
        }
        all_arguments.extend(arguments);
        receiver = inner_receiver;
    }
    let input = local(cx, receiver);
    if !stable_receiver(cx, receiver)
        || input.is_none() && evaluation_hazard(cx, receiver)
        || all_arguments
            .iter()
            .any(|argument| evaluation_hazard(cx, argument))
    {
        return None;
    }
    if style == Style::Mutable && input.is_none() {
        return None;
    }
    if let Some(input) = input
        && all_arguments
            .iter()
            .any(|argument| uses(cx, argument, input))
    {
        return None;
    }
    Some(Operation {
        input,
        output: None,
        input_ty: typeck(cx, receiver).expr_ty_adjusted(receiver),
        output_ty: typeck(cx, expr).expr_ty(expr),
        receiver: receiver.hir_id,
        binding: false,
        style,
        hir_id: expr.hir_id,
        span: expr.span,
    })
}

fn stable_receiver<'tcx>(cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) -> bool {
    if typeck(cx, expr)
        .expr_adjustments(expr)
        .iter()
        .any(|adjustment| {
            matches!(
                adjustment.kind,
                Adjust::Deref(DerefAdjustKind::Overloaded(_))
            )
        })
    {
        return false;
    }
    match expr.kind {
        ExprKind::AddrOf(_, _, inner) => stable_receiver(cx, inner),
        ExprKind::Unary(rustc_hir::UnOp::Deref, inner) => {
            matches!(typeck(cx, inner).expr_ty(inner).kind(), ty::Ref(..))
                && stable_receiver(cx, inner)
        }
        _ => true,
    }
}

fn single_use<'tcx>(cx: &LateContext<'tcx>, binding: HirId, receiver: HirId) -> bool {
    struct Uses<'a, 'tcx> {
        cx: &'a LateContext<'tcx>,
        binding: HirId,
        receiver: HirId,
        only_receiver: bool,
    }
    impl<'tcx> Visitor<'tcx> for Uses<'_, 'tcx> {
        fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
            if matches!(expr.kind, ExprKind::Path(_)) && local(self.cx, expr) == Some(self.binding)
            {
                self.only_receiver &= expr.hir_id == self.receiver;
            }
            if let ExprKind::Closure(closure) = expr.kind {
                self.visit_expr(self.cx.tcx.hir_body(closure.body).value);
            } else {
                intravisit::walk_expr(self, expr);
            }
        }
    }
    let mut uses = Uses {
        cx,
        binding,
        receiver,
        only_receiver: true,
    };
    let body = cx
        .tcx
        .hir_body_owned_by(cx.tcx.hir_enclosing_body_owner(binding));
    uses.visit_expr(body.value);
    uses.only_receiver
}

fn evaluation_hazard<'tcx>(cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) -> bool {
    struct Temporaries<'a, 'tcx> {
        cx: &'a LateContext<'tcx>,
        found: bool,
    }
    impl<'tcx> Visitor<'tcx> for Temporaries<'_, 'tcx> {
        fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
            self.found |= matches!(expr.kind, ExprKind::InlineAsm(_))
                || matches!(expr.kind, ExprKind::Block(block, _)
                    if matches!(block.rules, rustc_hir::BlockCheckMode::UnsafeBlock(_)))
                || matches!(typeck(self.cx, expr).expr_ty(expr).kind(), ty::RawPtr(..))
                || unsafe_call(self.cx, expr);
            // Named values keep their existing drop scopes; temporaries may
            // survive until the end of the combined statement.
            if !matches!(expr.kind, ExprKind::Path(_) | ExprKind::Field(..))
                || local(self.cx, expr).is_none() && matches!(expr.kind, ExprKind::Path(_))
            {
                self.found |= typeck(self.cx, expr)
                    .expr_ty(expr)
                    .needs_drop(self.cx.tcx, self.cx.typing_env());
            }
            if !self.found && !matches!(expr.kind, ExprKind::Closure(_)) {
                intravisit::walk_expr(self, expr);
            }
        }
    }
    let mut temporaries = Temporaries { cx, found: false };
    temporaries.visit_expr(expr);
    temporaries.found
}

fn unsafe_call<'tcx>(cx: &LateContext<'tcx>, expr: &Expr<'tcx>) -> bool {
    let id = match expr.kind {
        ExprKind::MethodCall(..) => typeck(cx, expr).type_dependent_def_id(expr.hir_id),
        ExprKind::Call(callee, _) => match typeck(cx, callee).expr_ty(callee).kind() {
            ty::FnDef(id, _) => Some(*id),
            ty::FnPtr(..) => {
                return typeck(cx, callee)
                    .expr_ty(callee)
                    .fn_sig(cx.tcx)
                    .safety()
                    .is_unsafe();
            }
            _ => None,
        },
        _ => None,
    };
    id.is_some_and(|id| {
        cx.tcx
            .fn_sig(id)
            .instantiate_identity()
            .skip_norm_wip()
            .safety()
            .is_unsafe()
    })
}

fn uses<'tcx>(cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>, binding: HirId) -> bool {
    struct Uses<'a, 'tcx> {
        cx: &'a LateContext<'tcx>,
        binding: HirId,
        found: bool,
    }
    impl<'tcx> Visitor<'tcx> for Uses<'_, 'tcx> {
        fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
            self.found |= local(self.cx, expr) == Some(self.binding);
            if self.found {
                return;
            }
            if let ExprKind::Closure(closure) = expr.kind {
                self.visit_expr(self.cx.tcx.hir_body(closure.body).value);
            } else {
                intravisit::walk_expr(self, expr);
            }
        }
    }
    let mut visitor = Uses {
        cx,
        binding,
        found: false,
    };
    visitor.visit_expr(expr);
    visitor.found
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
        mutable,
        partial_chain,
        generic_trait,
        ufcs,
        type_alias,
        local_builder,
        tail_expression,
        nested_body,
        consuming,
        assignment,
        constructor,
        renamed_binding,
        consuming_tail,
        consuming_ufcs,
        already_chained,
        intervening_statement,
        different_receivers,
        shadowed_binding,
        branch_boundary,
        loop_boundary,
        terminal_method,
        ordinary_collection,
        scalar_methods,
        stored_reference,
        macro_expansion,
        allowed,
        allowed_statement,
        borrowed_argument,
        earlier_borrowed_argument,
        nested_borrowed_argument,
        deref_receiver,
        returned_alias,
        returned_branches,
        delegated_receiver,
        external_builder,
        child_receiver,
        argument_receiver,
        conditional_receiver,
        reassigned_receiver,
        aliased_receiver_storage,
        generic_default,
        trait_override,
        recursive_receiver,
        unavailable_body,
        dynamic_dispatch,
        effectful_deref,
        temporary_guard,
        temporary_match,
        named_guard,
        retained_binding,
        captured_binding,
        qualified_trait_calls,
        coupled_lifetime,
        bounded_lifetime,
        independent_lifetime,
        unsafe_argument,
        concrete_default,
        loop_receiver,
    );
}
