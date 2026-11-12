#![feature(rustc_private)]

extern crate rustc_ast;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

use rustc_ast::LitKind;
use rustc_errors::DiagDecorator;
use rustc_hir::{
    BinOpKind, BindingMode, Expr, ExprKind, HirId, Node, Pat, PatExprKind, PatKind, QPath,
    def::{DefKind, Res},
    def_id::DefId,
    intravisit::{self, Visitor},
};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::{self, Ty, TypeckResults};
use rustc_span::{DesugaringKind, Span, Symbol};

rustc_session::declare_lint! {
    /// Effect registrations already select operations. Carrying that selection
    /// through a shared message and branching on it again duplicates routing.
    pub INSLAG_STATE_TREE_REDUNDANT_EFFECT_FORWARDING,
    Deny,
    "redundant forwarding of state-tree effects through shared message types"
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Selector<'tcx> {
    message: Ty<'tcx>,
    field: Option<DefId>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Value {
    Variant(DefId),
    String(Symbol),
}

struct Registration<'tcx> {
    name: Symbol,
    span: Span,
    selector: Selector<'tcx>,
    value: Value,
}

struct Effect<'tcx> {
    name: Symbol,
    span: Span,
    input: ty::Instance<'tcx>,
}

struct Input<'tcx> {
    constructor: DefId,
    name: Symbol,
    selector: Selector<'tcx>,
    value: Value,
}

struct Dispatch<'tcx> {
    hir_id: HirId,
    span: Span,
    selector: Selector<'tcx>,
    values: Vec<Value>,
}

#[derive(Default)]
struct RedundantEffectForwarding<'tcx> {
    effects: Vec<Effect<'tcx>>,
    inputs: Vec<Input<'tcx>>,
    dispatches: Vec<Dispatch<'tcx>>,
}

rustc_session::impl_lint_pass!(RedundantEffectForwarding<'_> => [INSLAG_STATE_TREE_REDUNDANT_EFFECT_FORWARDING]);
dylint_linting::dylint_library!();

#[unsafe(no_mangle)]
pub fn register_lints(_: &rustc_session::Session, store: &mut rustc_lint::LintStore) {
    store.register_lints(&[INSLAG_STATE_TREE_REDUNDANT_EFFECT_FORWARDING]);
    store.register_late_lint_pass(Box::new(|_| Box::new(RedundantEffectForwarding::default())));
}

impl<'tcx> LateLintPass<'tcx> for RedundantEffectForwarding<'tcx> {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        // Derived inputs route by effect name too; excluding expansion here would
        // make the derive an escape hatch from the same rule as handwritten inputs.
        if let ExprKind::Match(subject, arms, _) = expr.kind
            && effect_name(cx, subject)
            && arms.iter().all(|arm| arm.guard.is_none())
            && let Some(constructor) = input_constructor(cx, expr)
        {
            for arm in arms {
                let Some(output) = constructor_output(cx, arm.body) else {
                    continue;
                };
                for name in effect_names(cx, arm.pat) {
                    for (selector, value) in injected_values(cx, output) {
                        self.inputs.push(Input {
                            constructor,
                            name,
                            selector,
                            value,
                        });
                    }
                }
            }
        }
        if expr.span.from_expansion() {
            return;
        }
        if let Some((name, input)) = effect_registration(cx, expr)
            && let Some(Value::String(name_value)) = constant(cx, name, 0)
        {
            self.effects.push(Effect {
                name: name_value,
                span: name.span,
                input,
            });
        }

        let (subject, values) = match expr.kind {
            ExprKind::Match(subject, arms, _) => (
                subject,
                arms.iter()
                    .flat_map(|arm| pattern_values(cx, arm.pat))
                    .collect(),
            ),
            ExprKind::Let(binding) => (binding.init, pattern_values(cx, binding.pat)),
            ExprKind::Binary(op, left, right)
                if matches!(op.node, BinOpKind::Eq | BinOpKind::Ne) =>
            {
                if let Some(value) = constant(cx, right, 0) {
                    (left, vec![value])
                } else if let Some(value) = constant(cx, left, 0) {
                    (right, vec![value])
                } else {
                    return;
                }
            }
            _ => return,
        };
        if !values.is_empty()
            && let Some(selector) = selector(cx, subject, 0)
            && read_message(cx, subject, 0) == Some(selector.message)
        {
            self.dispatches.push(Dispatch {
                hir_id: expr.hir_id,
                span: expr.span,
                selector,
                values,
            });
        }
    }

    fn check_crate_post(&mut self, cx: &LateContext<'tcx>) {
        // Consumers can precede registrations or live in a different module.
        // Keep the originating HIR node so local lint levels still apply.
        let mut registrations = Vec::new();
        for effect in &self.effects {
            for input in &self.inputs {
                if input.constructor == effect.input.def_id() && input.name == effect.name {
                    registrations.push(Registration {
                        name: effect.name,
                        span: effect.span,
                        selector: Selector {
                            message: ty::EarlyBinder::bind(cx.tcx, input.selector.message)
                                .instantiate(cx.tcx, effect.input.args)
                                .skip_norm_wip(),
                            field: input.selector.field,
                        },
                        value: input.value,
                    });
                }
            }
        }
        registrations.sort_by_key(|registration| registration.span.lo());
        for dispatch in &self.dispatches {
            let Some(first) = registrations.iter().find(|registration| {
                registration.selector == dispatch.selector
                    && dispatch.values.contains(&registration.value)
            }) else {
                continue;
            };
            let Some(second) = registrations.iter().find(|registration| {
                registration.selector == first.selector
                    && registration.name != first.name
                    && registration.value != first.value
            }) else {
                continue;
            };
            cx.tcx.emit_node_span_lint(
                INSLAG_STATE_TREE_REDUNDANT_EFFECT_FORWARDING,
                dispatch.hir_id,
                dispatch.span,
                DiagDecorator(|diag| {
                    diag.primary_message(format!(
                        "redundant forwarding of effects `{}` and `{}` through a shared message type",
                        first.name, second.name,
                    ));
                    diag.span_note(first.span, "this effect fixes one discriminator value");
                    diag.span_note(second.span, "this effect fixes a different discriminator value");
                    diag.help(
                        "give each operation its own message type and consume it with `MessageReader<T>`",
                    );
                }),
            );
        }
    }
}

fn typeck<'tcx>(cx: &LateContext<'tcx>, id: HirId) -> &'tcx TypeckResults<'tcx> {
    cx.tcx.typeck(cx.tcx.hir_enclosing_body_owner(id))
}

fn expr_ty<'tcx>(cx: &LateContext<'tcx>, expr: &Expr<'tcx>) -> Ty<'tcx> {
    cx.tcx
        .erase_and_anonymize_regions(typeck(cx, expr.hir_id).expr_ty(expr).peel_refs())
}

fn path_def(cx: &LateContext<'_>, path: &QPath<'_>, id: HirId) -> Option<DefId> {
    typeck(cx, id).qpath_res(path, id).opt_def_id()
}

fn effect_registration<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Option<(&'tcx Expr<'tcx>, ty::Instance<'tcx>)> {
    let (method, generics, arguments) = match expr.kind {
        ExprKind::MethodCall(_, _, arguments, _) => (
            typeck(cx, expr.hir_id).type_dependent_def_id(expr.hir_id)?,
            typeck(cx, expr.hir_id).node_args(expr.hir_id),
            arguments,
        ),
        ExprKind::Call(callee, [_, arguments @ ..]) => {
            let ty::FnDef(id, generics) = typeck(cx, callee.hir_id).expr_ty(callee).kind() else {
                return None;
            };
            (*id, generics.skip_binder(), arguments)
        }
        _ => return None,
    };
    if cx.tcx.item_name(method).as_str() != "add_state_tree_effect" {
        return None;
    }
    let parent = cx.tcx.parent(method);
    let owner = match cx.tcx.def_kind(parent) {
        DefKind::Trait => parent,
        DefKind::Impl { of_trait: true } => {
            cx.tcx
                .impl_trait_ref(parent)
                .instantiate_identity()
                .skip_norm_wip()
                .def_id
        }
        _ => return None,
    };
    if definition(cx, owner, "StateTreeExt") != Some(owner) {
        return None;
    }
    let input = definition(cx, owner, "StateTreeInput")?;
    let constructor = cx
        .tcx
        .associated_items(input)
        .in_definition_order()
        .find(|item| item.name().as_str() == "from_state_tree_input")?
        .def_id;
    let message = generics.type_at(cx.tcx.generics_of(method).parent_count);
    let input = ty::Instance::try_resolve(
        cx.tcx,
        cx.typing_env().with_post_analysis_normalized(cx.tcx),
        constructor,
        cx.tcx.mk_args(&[message.into()]),
    )
    .ok()??;
    let [name] = arguments else {
        return None;
    };
    Some((name, input))
}

fn definition(cx: &LateContext<'_>, owner: DefId, name: &str) -> Option<DefId> {
    if cx.tcx.crate_name(owner.krate).as_str() != "inslag_state_tree" {
        return None;
    }
    let root = DefId {
        krate: owner.krate,
        index: rustc_hir::def_id::CRATE_DEF_INDEX,
    };
    let children = if let Some(root) = root.as_local() {
        cx.tcx.module_children_local(root)
    } else {
        cx.tcx.module_children(root)
    };
    children
        .iter()
        .find_map(|child| (child.ident.name.as_str() == name).then(|| child.res.opt_def_id())?)
}

fn input_constructor<'tcx>(cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) -> Option<DefId> {
    let owner = cx.tcx.hir_enclosing_body_owner(expr.hir_id);
    let item = cx
        .tcx
        .opt_associated_item(owner.to_def_id())?
        .trait_item_def_id()?;
    if cx.tcx.item_name(item).as_str() != "from_state_tree_input"
        || definition(cx, item, "StateTreeInput") != Some(cx.tcx.parent(item))
    {
        return None;
    }
    let body = cx.tcx.hir_body_owned_by(owner).value;
    if peel(cx, body, 0).hir_id != expr.hir_id {
        return None;
    }
    // A routing expression that is discarded or bypassed by an early return
    // does not prove which discriminator a registered effect will emit.
    let mut returns = ExplicitReturns(false);
    returns.visit_expr(body);
    (!returns.0).then_some(owner.to_def_id())
}

fn effect_name<'tcx>(cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) -> bool {
    let expr = peel(cx, expr, 0);
    let ExprKind::MethodCall(_, receiver, [], _) = expr.kind else {
        return false;
    };
    let Some(method) = typeck(cx, expr.hir_id).type_dependent_def_id(expr.hir_id) else {
        return false;
    };
    let ty::Adt(adt, _) = expr_ty(cx, receiver).kind() else {
        return false;
    };
    cx.tcx.item_name(method).as_str() == "effect_name"
        && definition(cx, method, "StateTreeContext") == Some(adt.did())
}

fn effect_names(cx: &LateContext<'_>, pat: &Pat<'_>) -> Vec<Symbol> {
    match pat.kind {
        PatKind::TupleStruct(ref path, [inner], _)
            if path_def(cx, path, pat.hir_id).and_then(|id| variant_id(cx, id))
                == cx.tcx.lang_items().option_some_variant() =>
        {
            pattern_values(cx, inner)
                .into_iter()
                .filter_map(|value| match value {
                    Value::String(name) => Some(name),
                    _ => None,
                })
                .collect()
        }
        PatKind::Or(patterns) => patterns
            .iter()
            .flat_map(|pat| effect_names(cx, pat))
            .collect(),
        _ => Vec::new(),
    }
}

fn local_initializer<'tcx>(cx: &LateContext<'tcx>, id: HirId) -> Option<&'tcx Expr<'tcx>> {
    let Node::Pat(pat) = cx.tcx.hir_node(id) else {
        return None;
    };
    if !matches!(pat.kind, PatKind::Binding(BindingMode::NONE, _, _, None)) {
        return None;
    }
    let Node::LetStmt(binding) = cx.tcx.parent_hir_node(id) else {
        return None;
    };
    binding.init
}

fn pattern_source<'tcx>(cx: &LateContext<'tcx>, id: HirId) -> Option<&'tcx Expr<'tcx>> {
    for (parent, node) in cx.tcx.hir_parent_iter(id) {
        match node {
            Node::Pat(_) | Node::PatField(_) => {}
            Node::Arm(_) => {
                let Node::Expr(expression) = cx.tcx.parent_hir_node(parent) else {
                    return None;
                };
                let ExprKind::Match(subject, _, _) = expression.kind else {
                    return None;
                };
                return Some(subject);
            }
            _ => return None,
        }
    }
    None
}

fn read_message<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    depth: usize,
) -> Option<Ty<'tcx>> {
    if depth >= 32 {
        return None;
    }
    let expr = peel(cx, expr, 0);
    match expr.kind {
        ExprKind::Path(ref path) => {
            let Res::Local(id) = typeck(cx, expr.hir_id).qpath_res(path, expr.hir_id) else {
                return None;
            };
            read_message(cx, pattern_source(cx, id)?, depth + 1)
        }
        ExprKind::Field(base, _) => read_message(cx, base, depth + 1),
        ExprKind::MethodCall(_, receiver, [], _) => {
            let id = typeck(cx, expr.hir_id).type_dependent_def_id(expr.hir_id)?;
            if cx.tcx.crate_name(id.krate).as_str() == "bevy_ecs"
                && matches!(cx.tcx.item_name(id).as_str(), "read" | "read_with_id")
            {
                let receiver_ty = typeck(cx, receiver.hir_id)
                    .expr_ty_adjusted(receiver)
                    .peel_refs();
                let ty::Adt(adt, arguments) = receiver_ty.kind() else {
                    return None;
                };
                let root = DefId {
                    krate: id.krate,
                    index: rustc_hir::def_id::CRATE_DEF_INDEX,
                };
                let module = cx.tcx.module_children(root).iter().find_map(|child| {
                    (child.ident.name.as_str() == "message").then(|| child.res.opt_def_id())?
                })?;
                if cx.tcx.module_children(module).iter().any(|child| {
                    child.ident.name.as_str() == "MessageReader"
                        && child.res.opt_def_id() == Some(adt.did())
                }) {
                    return arguments
                        .types()
                        .next()
                        .map(|ty| cx.tcx.erase_and_anonymize_regions(ty));
                }
                return None;
            }
            if preserving_iterator_call(cx, id) || string_conversion(cx, id, expr, receiver) {
                return read_message(cx, receiver, depth + 1);
            }
            None
        }
        ExprKind::Call(callee, [argument]) => {
            let ty::FnDef(id, _) = typeck(cx, callee.hir_id).expr_ty(callee).kind() else {
                return None;
            };
            if preserving_iterator_call(cx, *id) {
                return read_message(cx, argument, depth + 1);
            }
            None
        }
        _ => None,
    }
}

fn preserving_iterator_call(cx: &LateContext<'_>, id: DefId) -> bool {
    cx.tcx.crate_name(id.krate).as_str() == "core"
        && matches!(
            cx.tcx.item_name(id).as_str(),
            "into_iter" | "next" | "copied" | "enumerate"
        )
}

fn peel<'tcx>(cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>, depth: usize) -> &'tcx Expr<'tcx> {
    if depth >= 16 {
        return expr;
    }
    let inner = match expr.kind {
        ExprKind::Block(block, _) => block.expr,
        ExprKind::Ret(Some(inner)) | ExprKind::AddrOf(_, _, inner) => Some(inner),
        ExprKind::Unary(rustc_hir::UnOp::Deref, inner) => Some(inner),
        ExprKind::Path(ref path) => match typeck(cx, expr.hir_id).qpath_res(path, expr.hir_id) {
            Res::Local(id) => local_initializer(cx, id),
            _ => None,
        },
        _ => None,
    };
    inner.map_or(expr, |inner| peel(cx, inner, depth + 1))
}

fn constructor_output<'tcx>(
    cx: &LateContext<'tcx>,
    constructor: &'tcx Expr<'tcx>,
) -> Option<&'tcx Expr<'tcx>> {
    let body = constructor;
    let mut returns = ExplicitReturns(false);
    returns.visit_expr(body);
    if returns.0 {
        return None;
    }
    let output = peel(cx, body, 0);
    let ExprKind::Call(callee, [message]) = output.kind else {
        return None;
    };
    let ExprKind::Path(ref path) = callee.kind else {
        return None;
    };
    let id = path_def(cx, path, callee.hir_id)?;
    let variant = variant_id(cx, id)?;
    if cx.tcx.lang_items().result_ok_variant() != Some(variant) {
        return None;
    }
    Some(peel(cx, message, 0))
}

struct ExplicitReturns(bool);

impl<'tcx> Visitor<'tcx> for ExplicitReturns {
    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        // An early success return can contradict the constructor's tail value.
        // Desugared `?` returns only an error, so it does not weaken the evidence.
        if (matches!(expr.kind, ExprKind::Ret(_))
            && !expr.span.is_desugaring(DesugaringKind::QuestionMark))
            || matches!(expr.kind, ExprKind::Break(_, Some(_)))
        {
            self.0 = true;
        }
        intravisit::walk_expr(self, expr);
    }
}

fn variant_id(cx: &LateContext<'_>, id: DefId) -> Option<DefId> {
    match cx.tcx.def_kind(id) {
        DefKind::Variant => Some(id),
        DefKind::Ctor(..) => {
            let parent = cx.tcx.parent(id);
            (cx.tcx.def_kind(parent) == DefKind::Variant).then_some(parent)
        }
        _ => None,
    }
}

fn constant<'tcx>(cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>, depth: usize) -> Option<Value> {
    if depth >= 16 {
        return None;
    }
    let expr = peel(cx, expr, depth);
    match expr.kind {
        ExprKind::Lit(literal) => match literal.node {
            LitKind::Str(value, _) => Some(Value::String(value)),
            _ => None,
        },
        ExprKind::Path(ref path) => {
            variant_id(cx, path_def(cx, path, expr.hir_id)?).map(Value::Variant)
        }
        ExprKind::Call(callee, arguments) => {
            let ExprKind::Path(ref path) = callee.kind else {
                return None;
            };
            let id = path_def(cx, path, callee.hir_id)?;
            if let Some(variant) = variant_id(cx, id) {
                return Some(Value::Variant(variant));
            }
            if let [argument] = arguments
                && string_conversion(cx, id, expr, argument)
            {
                return constant(cx, argument, depth + 1);
            }
            None
        }
        ExprKind::Struct(path, _, _) => {
            variant_id(cx, path_def(cx, path, expr.hir_id)?).map(Value::Variant)
        }
        ExprKind::MethodCall(_, receiver, [], _) => {
            let id = typeck(cx, expr.hir_id).type_dependent_def_id(expr.hir_id)?;
            string_conversion(cx, id, expr, receiver).then(|| constant(cx, receiver, depth + 1))?
        }
        _ => None,
    }
}

fn is_string(cx: &LateContext<'_>, ty: Ty<'_>) -> bool {
    ty.is_str()
        || matches!(ty.kind(), ty::Adt(adt, _) if Some(adt.did()) == cx.tcx.lang_items().string())
}

fn string_conversion<'tcx>(
    cx: &LateContext<'tcx>,
    id: DefId,
    output: &Expr<'tcx>,
    input: &Expr<'tcx>,
) -> bool {
    // Looking only at method spelling would treat user-defined transformations
    // as identity conversions and manufacture evidence that is not constant.
    matches!(cx.tcx.crate_name(id.krate).as_str(), "alloc" | "core")
        && matches!(
            cx.tcx.item_name(id).as_str(),
            "into" | "from" | "to_owned" | "to_string" | "clone" | "as_str" | "as_ref"
        )
        && is_string(cx, expr_ty(cx, input))
        && is_string(cx, expr_ty(cx, output))
}

fn injected_values<'tcx>(
    cx: &LateContext<'tcx>,
    output: &'tcx Expr<'tcx>,
) -> Vec<(Selector<'tcx>, Value)> {
    let message = expr_ty(cx, output);
    let ty::Adt(adt, _) = message.kind() else {
        return Vec::new();
    };
    if adt.is_enum() {
        return constant(cx, output, 0)
            .map(|value| {
                (
                    Selector {
                        message,
                        field: None,
                    },
                    value,
                )
            })
            .into_iter()
            .collect();
    }
    if adt.is_struct()
        && let ExprKind::Call(callee, arguments) = output.kind
        && let ExprKind::Path(ref path) = callee.kind
        && let Some(id) = path_def(cx, path, callee.hir_id)
        && matches!(cx.tcx.def_kind(id), DefKind::Ctor(..))
        && cx.tcx.parent(id) == adt.did()
    {
        return arguments
            .iter()
            .zip(&adt.non_enum_variant().fields)
            .filter_map(|(argument, field)| {
                Some((
                    Selector {
                        message,
                        field: Some(field.did),
                    },
                    constant(cx, argument, 0)?,
                ))
            })
            .collect();
    }
    let ExprKind::Struct(_, fields, _) = output.kind else {
        return Vec::new();
    };
    fields
        .iter()
        .filter_map(|field| {
            let value = constant(cx, field.expr, 0)?;
            let index = typeck(cx, output.hir_id).opt_field_index(field.hir_id)?;
            Some((
                Selector {
                    message,
                    field: Some(adt.non_enum_variant().fields.get(index)?.did),
                },
                value,
            ))
        })
        .collect()
}

fn selector<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    depth: usize,
) -> Option<Selector<'tcx>> {
    if depth >= 16 {
        return None;
    }
    let expr = peel(cx, expr, depth);
    match expr.kind {
        ExprKind::Field(base, _) => {
            let message = cx.tcx.erase_and_anonymize_regions(
                typeck(cx, base.hir_id).expr_ty_adjusted(base).peel_refs(),
            );
            let ty::Adt(adt, _) = message.kind() else {
                return None;
            };
            if !adt.is_struct() {
                return None;
            }
            let index = typeck(cx, expr.hir_id).opt_field_index(expr.hir_id)?;
            Some(Selector {
                message,
                field: Some(adt.non_enum_variant().fields.get(index)?.did),
            })
        }
        ExprKind::MethodCall(_, receiver, [], _) => {
            let id = typeck(cx, expr.hir_id).type_dependent_def_id(expr.hir_id)?;
            string_conversion(cx, id, expr, receiver).then(|| selector(cx, receiver, depth + 1))?
        }
        _ => {
            let message = expr_ty(cx, expr);
            if let ty::Adt(adt, _) = message.kind()
                && adt.is_enum()
            {
                return Some(Selector {
                    message,
                    field: None,
                });
            }
            None
        }
    }
}

fn pattern_values(cx: &LateContext<'_>, pat: &Pat<'_>) -> Vec<Value> {
    match pat.kind {
        PatKind::Expr(expression) => match expression.kind {
            PatExprKind::Lit { lit, .. } => match lit.node {
                LitKind::Str(value, _) => vec![Value::String(value)],
                _ => Vec::new(),
            },
            PatExprKind::Path(ref path) => path_def(cx, path, expression.hir_id)
                .and_then(|id| variant_id(cx, id))
                .map(Value::Variant)
                .into_iter()
                .collect(),
        },
        PatKind::Struct(ref path, _, _) | PatKind::TupleStruct(ref path, _, _) => {
            path_def(cx, path, pat.hir_id)
                .and_then(|id| variant_id(cx, id))
                .map(Value::Variant)
                .into_iter()
                .collect()
        }
        PatKind::Ref(inner, _, _) | PatKind::Binding(_, _, _, Some(inner)) => {
            pattern_values(cx, inner)
        }
        PatKind::Or(patterns) => patterns
            .iter()
            .flat_map(|pat| pattern_values(cx, pat))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn local_definitions() {
        dylint_testing::ui::Test::example(env!("CARGO_PKG_NAME"), "local_definitions")
            .rustc_flags(["--crate-name=inslag_state_tree"])
            .run();
    }

    macro_rules! examples {
        ($($name:ident),+ $(,)?) => {
            $(#[test]
            fn $name() {
                dylint_testing::ui_test_example(env!("CARGO_PKG_NAME"), stringify!($name));
            })+
        };
    }

    examples!(
        enum_forwarding,
        enum_conditions,
        string_forwarding,
        string_conditions,
        borrowed_forwarding,
        resolved_paths,
        iterator_readers,
        lint_levels,
        enum_fields,
        tuple_forwarding,
        capability_readers,
        domain_payload,
        effect_aliases,
        payload_fields,
        repeated_name,
        dynamic_constructor,
        custom_conversion,
        dynamic_selector,
        generic_messages,
        generic_forwarding,
        unregistered_variants,
        discarded_inputs,
        guarded_inputs,
        mutable_constructor,
        macro_return,
        labeled_break,
        unrelated_api,
        non_consumers,
        transformed_messages,
        constructor_factory,
        unrelated_messages,
        type_expressions,
        deref_fields
    );
}
