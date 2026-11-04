#![feature(rustc_private)]

extern crate rustc_ast;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

use std::collections::{HashMap, HashSet};

use rustc_errors::DiagDecorator;
use rustc_hir::{
    BindingMode, Body, Expr, ExprKind, HirId, Node, Pat, PatKind,
    def::Res,
    def_id::{DefId, LocalDefId},
    intravisit::{self, Visitor},
};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::adjustment::{Adjust, DerefAdjustKind};
use rustc_middle::ty::{self, Ty, TypeckResults};
use rustc_span::{Span, Symbol};

rustc_session::declare_lint! {
    /// Unknown effects, dependencies, or escaping results cannot establish waste.
    pub DISCARDED_PROJECTION_WORK,
    Deny,
    "constructing result fields that this caller does not use"
}

#[derive(Default)]
struct DiscardedProjectionWork {
    pure: HashMap<LocalDefId, bool>,
}

rustc_session::impl_lint_pass!(DiscardedProjectionWork => [DISCARDED_PROJECTION_WORK]);
dylint_linting::dylint_library!();

#[unsafe(no_mangle)]
pub fn register_lints(_: &rustc_session::Session, store: &mut rustc_lint::LintStore) {
    store.register_lints(&[DISCARDED_PROJECTION_WORK]);
    store.register_renamed(
        "bevy_discarded_state_projection",
        "discarded_projection_work",
    );
    store.register_late_lint_pass(Box::new(|_| Box::new(DiscardedProjectionWork::default())));
}

impl<'tcx> LateLintPass<'tcx> for DiscardedProjectionWork {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion() {
            return;
        }
        let Some((callee, arguments)) = call(cx, expr) else {
            return;
        };
        let Some(local) = callee.as_local() else {
            return;
        };
        let Some(body) = cx.tcx.hir_maybe_body_owned_by(local) else {
            return;
        };
        let inputs: HashMap<_, _> = body
            .params
            .iter()
            .zip(&arguments)
            .filter_map(|(param, arg)| simple_binding(param.pat).map(|binding| (binding, *arg)))
            .collect();
        let sources: HashSet<_> = inputs.keys().copied().collect();
        if sources.is_empty() {
            return;
        }
        let Some((projections, output)) = projections(cx, body) else {
            return;
        };
        if !safe_drop(cx, output, &mut HashSet::new()) {
            return;
        }
        let Some(used) = result_uses(cx, expr) else {
            return;
        };
        if used.is_empty() {
            return;
        }
        let all_fields: Vec<_> = projections.iter().flatten().copied().collect();
        let mut discarded = Vec::new();
        for fields in &projections {
            for (name, value) in fields {
                if !used.contains(name)
                    && depends_on(cx, value, &sources)
                    && independent(cx, value, &all_fields, &used)
                    && let Some(work) = construction(cx, value, &inputs, &mut HashSet::new())
                {
                    discarded.push((*name, value.span, work));
                }
            }
        }
        if discarded.is_empty() || !self.pure_function(cx, local) {
            return;
        }
        let mut used: Vec<_> = used.into_iter().map(|name| name.to_string()).collect();
        used.sort();
        let names = discarded
            .iter()
            .map(|(name, _, _)| name.to_string())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        cx.emit_span_lint(
            DISCARDED_PROJECTION_WORK,
            expr.span,
            DiagDecorator(|diag| {
                diag.primary_message(format!("projection builds unused fields: {names}"));
                diag.note(format!(
                    "this caller uses only these fields: {}",
                    used.join(", ")
                ));
                for (name, span, work) in &discarded {
                    diag.span_note(
                        *span,
                        format!("`{name}` is built here but not used by this caller"),
                    );
                    if *span != work.span {
                        diag.span_note(work.span, work.reason);
                    } else {
                        diag.note(work.reason);
                    }
                }
                diag.help("compute or capture only the values this caller needs");
            }),
        );
    }
}

fn typeck<'tcx>(cx: &LateContext<'tcx>, id: HirId) -> &'tcx TypeckResults<'tcx> {
    cx.tcx.typeck(cx.tcx.hir_enclosing_body_owner(id))
}

fn expr_ty<'tcx>(cx: &LateContext<'tcx>, expr: &Expr<'tcx>) -> Ty<'tcx> {
    typeck(cx, expr.hir_id).expr_ty(expr)
}

fn call<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Option<(DefId, Vec<&'tcx Expr<'tcx>>)> {
    match expr.kind {
        ExprKind::MethodCall(_, receiver, args, _) => Some((
            typeck(cx, expr.hir_id).type_dependent_def_id(expr.hir_id)?,
            std::iter::once(receiver).chain(args).collect(),
        )),
        ExprKind::Call(callee, args) => {
            let ty::FnDef(id, _) = expr_ty(cx, callee).kind() else {
                return None;
            };
            Some((*id, args.iter().collect()))
        }
        _ => None,
    }
}

fn simple_binding(pat: &Pat<'_>) -> Option<HirId> {
    match pat.kind {
        PatKind::Binding(BindingMode::NONE, id, _, None) => Some(id),
        _ => None,
    }
}

fn local_path(cx: &LateContext<'_>, expr: &Expr<'_>) -> Option<HirId> {
    if let ExprKind::Path(ref path) = expr.kind
        && let Res::Local(id) = typeck(cx, expr.hir_id).qpath_res(path, expr.hir_id)
    {
        Some(id)
    } else {
        None
    }
}

fn initializer<'tcx>(cx: &LateContext<'tcx>, id: HirId) -> Option<&'tcx Expr<'tcx>> {
    let Node::Pat(pat) = cx.tcx.hir_node(id) else {
        return None;
    };
    simple_binding(pat)?;
    let Node::LetStmt(binding) = cx.tcx.parent_hir_node(id) else {
        return None;
    };
    if binding.els.is_some() {
        return None;
    }
    binding.init
}

fn tail<'tcx>(mut expr: &'tcx Expr<'tcx>) -> &'tcx Expr<'tcx> {
    loop {
        match expr.kind {
            ExprKind::Block(block, _) if block.expr.is_some() => expr = block.expr.unwrap(),
            ExprKind::Ret(Some(value)) => expr = value,
            _ => return expr,
        }
    }
}

fn return_values<'tcx>(body: &'tcx Body<'tcx>) -> Vec<&'tcx Expr<'tcx>> {
    struct Returns<'tcx>(Vec<&'tcx Expr<'tcx>>);
    impl<'tcx> Visitor<'tcx> for Returns<'tcx> {
        fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
            match expr.kind {
                ExprKind::Ret(Some(value)) => self.0.push(value),
                ExprKind::Closure(_) => {}
                _ => intravisit::walk_expr(self, expr),
            }
        }
    }
    let mut returns = Returns(vec![tail(body.value)]);
    returns.visit_expr(body.value);
    returns.0
}

type ProjectionFields<'tcx> = Vec<(Symbol, &'tcx Expr<'tcx>)>;

fn projections<'tcx>(
    cx: &LateContext<'tcx>,
    body: &'tcx Body<'tcx>,
) -> Option<(Vec<ProjectionFields<'tcx>>, Ty<'tcx>)> {
    let mut seen = HashSet::new();
    let mut projections = Vec::new();
    let output = expr_ty(cx, tail(body.value));
    for value in return_values(body) {
        let value = tail(value);
        if !seen.insert(value.hir_id) {
            continue;
        }
        if expr_ty(cx, value) != output {
            return None;
        }
        match value.kind {
            ExprKind::Struct(_, fields, rustc_hir::StructTailExpr::None) if matches!(output.kind(), ty::Adt(adt, _) if adt.is_struct() && adt.did().is_local()) =>
            {
                projections.push(
                    fields
                        .iter()
                        .map(|field| (field.ident.name, field.expr))
                        .collect(),
                );
            }
            ExprKind::Tup(fields) => projections.push(
                fields
                    .iter()
                    .enumerate()
                    .map(|(index, value)| (Symbol::intern(&index.to_string()), value))
                    .collect(),
            ),
            _ => return None,
        }
    }
    Some((projections, output))
}

fn is_vec(cx: &LateContext<'_>, ty: Ty<'_>) -> bool {
    matches!(ty.kind(), ty::Adt(adt, args) if definition_path(cx, adt.did()) == "alloc::vec::Vec"
        && args.types().nth(1).is_none_or(|allocator| matches!(allocator.kind(), ty::Adt(adt, _) if definition_path(cx, adt.did()) == "alloc::alloc::Global")))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Collection {
    Vec,
    String,
    Deque,
    List,
    Map,
    Set,
    Heap,
}

fn collection(cx: &LateContext<'_>, ty: Ty<'_>) -> Option<Collection> {
    let ty::Adt(adt, args) = ty.peel_refs().kind() else {
        return None;
    };
    let (kind, elements) = match definition_path(cx, adt.did()).as_str() {
        "alloc::vec::Vec" => (Collection::Vec, 1),
        "alloc::string::String" => (Collection::String, 0),
        "alloc::collections::vec_deque::VecDeque" => (Collection::Deque, 1),
        "alloc::collections::linked_list::LinkedList" => (Collection::List, 1),
        "alloc::collections::btree::map::BTreeMap" => (Collection::Map, 2),
        "alloc::collections::btree::set::BTreeSet" => (Collection::Set, 1),
        "alloc::collections::binary_heap::BinaryHeap" => (Collection::Heap, 1),
        _ => return None,
    };
    args.types().nth(elements).is_none_or(|allocator| {
        matches!(allocator.kind(), ty::Adt(adt, _) if definition_path(cx, adt.did()) == "alloc::alloc::Global")
    }).then_some(kind)
}

fn collection_method(path: &str) -> bool {
    [
        "alloc::vec::",
        "alloc::string::",
        "alloc::collections::vec_deque::",
        "alloc::collections::linked_list::",
        "alloc::collections::btree::",
        "alloc::collections::binary_heap::",
    ]
    .iter()
    .any(|prefix| path.starts_with(prefix))
}

fn collection_allocates<'tcx>(cx: &LateContext<'tcx>, ty: Ty<'tcx>) -> bool {
    match collection(cx, ty) {
        Some(Collection::Vec | Collection::Deque | Collection::Heap) => {
            let ty::Adt(_, args) = ty.peel_refs().kind() else {
                return false;
            };
            args.types().next().is_some_and(|element| {
                cx.tcx
                    .layout_of(cx.typing_env().as_query_input(element))
                    .is_ok_and(|layout| !layout.is_zst())
            })
        }
        Some(_) => true,
        None => false,
    }
}

fn definition_path(cx: &LateContext<'_>, id: DefId) -> String {
    format!(
        "{}::{}",
        cx.tcx.crate_name(id.krate),
        cx.tcx
            .def_path(id)
            .to_string_no_crate_verbose()
            .trim_start_matches("::")
    )
}

fn safe_drop<'tcx>(cx: &LateContext<'tcx>, ty: Ty<'tcx>, seen: &mut HashSet<Ty<'tcx>>) -> bool {
    if !seen.insert(ty) {
        return true;
    }
    match ty.kind() {
        ty::Bool
        | ty::Char
        | ty::Int(_)
        | ty::Uint(_)
        | ty::Float(_)
        | ty::Never
        | ty::Str
        | ty::Ref(..)
        | ty::RawPtr(..)
        | ty::FnDef(..)
        | ty::FnPtr(..) => true,
        ty::Tuple(types) => types.iter().all(|ty| safe_drop(cx, ty, seen)),
        ty::Array(ty, _) | ty::Slice(ty) | ty::Pat(ty, _) => safe_drop(cx, *ty, seen),
        ty::Adt(_, args) if collection(cx, ty).is_some() => {
            args.types().all(|ty| safe_drop(cx, ty, seen))
        }
        ty::Adt(adt, args) => {
            adt.destructor(cx.tcx).is_none()
                && adt
                    .all_fields()
                    .all(|field| safe_drop(cx, field.ty(cx.tcx, args).skip_norm_wip(), seen))
        }
        _ => false,
    }
}

fn depends_on<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    sources: &HashSet<HirId>,
) -> bool {
    dependencies(cx, expr, sources, &mut HashSet::new())
}

fn dependencies<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    sources: &HashSet<HirId>,
    calls: &mut HashSet<LocalDefId>,
) -> bool {
    struct Dependencies<'a, 'tcx> {
        cx: &'a LateContext<'tcx>,
        sources: &'a HashSet<HirId>,
        seen: HashSet<HirId>,
        found: bool,
        calls: &'a mut HashSet<LocalDefId>,
    }
    impl<'tcx> Visitor<'tcx> for Dependencies<'_, 'tcx> {
        fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
            if let Some((id, args)) = call(self.cx, expr)
                && let Some(local) = id.as_local()
                && let Some(body) = self.cx.tcx.hir_maybe_body_owned_by(local)
            {
                if self.calls.insert(local) {
                    for (arg, param) in args.iter().zip(body.params) {
                        if let Some(binding) = simple_binding(param.pat)
                            && dependencies(self.cx, arg, self.sources, self.calls)
                            && return_depends_on(self.cx, body, binding, self.calls)
                        {
                            self.found = true;
                        }
                    }
                    self.calls.remove(&local);
                }
                return;
            }
            if let Some(id) = local_path(self.cx, expr) {
                if self.sources.contains(&id) {
                    self.found = true;
                } else if self.seen.insert(id) {
                    for input in collection_inputs(self.cx, id, true) {
                        self.visit_expr(input);
                    }
                }
            }
            if let ExprKind::Block(block, _) = expr.kind {
                if let Some(value) = block.expr {
                    self.visit_expr(value);
                }
            } else if let ExprKind::Closure(closure) = expr.kind {
                self.visit_expr(self.cx.tcx.hir_body(closure.body).value);
            } else {
                intravisit::walk_expr(self, expr);
            }
        }
    }
    let mut visitor = Dependencies {
        cx,
        sources,
        seen: HashSet::new(),
        found: false,
        calls,
    };
    visitor.visit_expr(expr);
    visitor.found
}

fn return_depends_on<'tcx>(
    cx: &LateContext<'tcx>,
    body: &'tcx Body<'tcx>,
    binding: HirId,
    calls: &mut HashSet<LocalDefId>,
) -> bool {
    let sources = HashSet::from([binding]);
    return_values(body).iter().any(|value| {
        dependencies(cx, value, &sources, calls)
            || cx.tcx.hir_parent_iter(value.hir_id).any(|(_, node)| {
                matches!(node, Node::Expr(parent)
                    if matches!(parent.kind, ExprKind::If(..) | ExprKind::Match(..))
                        && dependencies(cx, parent, &sources, calls))
            })
    })
}

struct Construction {
    span: Span,
    reason: &'static str,
}

fn construction<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    inputs: &HashMap<HirId, &'tcx Expr<'tcx>>,
    seen: &mut HashSet<HirId>,
) -> Option<Construction> {
    struct Work<'a, 'tcx> {
        cx: &'a LateContext<'tcx>,
        seen: &'a mut HashSet<HirId>,
        inputs: &'a HashMap<HirId, &'tcx Expr<'tcx>>,
        found: Option<Construction>,
    }
    impl<'tcx> Visitor<'tcx> for Work<'_, 'tcx> {
        fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
            if self.found.is_some() || !self.seen.insert(expr.hir_id) {
                return;
            }
            if let Some((id, args)) = call(self.cx, expr) {
                let path = definition_path(self.cx, id);
                let name = self.cx.tcx.item_name(id);
                let reason = if collection_allocates(self.cx, expr_ty(self.cx, expr))
                    && (path == "core::clone::Clone::clone"
                        || path == "alloc::borrow::ToOwned::to_owned"
                        || (path.starts_with("alloc::") && name.as_str() == "to_vec"))
                    && args.first().is_some_and(|arg| {
                        !definitely_empty(self.cx, arg, self.inputs, &mut HashSet::new())
                    }) {
                    Some("this copy only builds an unused field")
                } else if path.starts_with("core::iter::")
                    && name.as_str() == "collect"
                    && collection_allocates(self.cx, expr_ty(self.cx, expr))
                    && args.first().is_some_and(|arg| {
                        !definitely_empty(self.cx, arg, self.inputs, &mut HashSet::new())
                    })
                {
                    Some("this iterator is collected only to build an unused field")
                } else if collection_method(&path)
                    && matches!(
                        name.as_str(),
                        "push" | "push_back" | "push_front" | "push_str" | "insert"
                    )
                    && args
                        .first()
                        .is_some_and(|arg| collection_allocates(self.cx, expr_ty(self.cx, arg)))
                {
                    Some("this insertion only builds an unused field")
                } else {
                    None
                };
                if let Some(reason) = reason {
                    self.found = Some(Construction {
                        span: expr.span,
                        reason,
                    });
                    return;
                }
                if let Some(local) = id.as_local()
                    && let Some(body) = self.cx.tcx.hir_maybe_body_owned_by(local)
                {
                    for value in return_values(body) {
                        self.visit_expr(value);
                    }
                }
            }
            if let Some(id) = local_path(self.cx, expr) {
                for input in collection_inputs(self.cx, id, false) {
                    self.visit_expr(input);
                }
            }
            if let ExprKind::Block(block, _) = expr.kind {
                if let Some(value) = block.expr {
                    self.visit_expr(value);
                }
            } else if let ExprKind::Closure(closure) = expr.kind {
                self.visit_expr(self.cx.tcx.hir_body(closure.body).value);
            } else {
                intravisit::walk_expr(self, expr);
            }
        }
    }
    let mut visitor = Work {
        cx,
        seen,
        inputs,
        found: None,
    };
    visitor.visit_expr(expr);
    visitor.found
}

fn resolved<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    inputs: &HashMap<HirId, &'tcx Expr<'tcx>>,
    seen: &mut HashSet<HirId>,
) -> &'tcx Expr<'tcx> {
    if !seen.insert(expr.hir_id) {
        return expr;
    }
    match tail(expr).kind {
        ExprKind::Path(_) => local_path(cx, expr)
            .and_then(|binding| {
                inputs
                    .get(&binding)
                    .copied()
                    .or_else(|| initializer(cx, binding))
            })
            .map_or(expr, |value| resolved(cx, value, inputs, seen)),
        ExprKind::AddrOf(_, _, base) | ExprKind::Unary(rustc_hir::UnOp::Deref, base) => {
            resolved(cx, base, inputs, seen)
        }
        ExprKind::Field(base, name) => {
            if let ExprKind::Struct(_, fields, rustc_hir::StructTailExpr::None) =
                resolved(cx, base, inputs, seen).kind
                && let Some(field) = fields.iter().find(|field| field.ident.name == name.name)
            {
                resolved(cx, field.expr, inputs, seen)
            } else {
                expr
            }
        }
        _ => expr,
    }
}

fn definitely_empty<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    inputs: &HashMap<HirId, &'tcx Expr<'tcx>>,
    seen: &mut HashSet<HirId>,
) -> bool {
    let expr = resolved(cx, expr, inputs, seen);
    match tail(expr).kind {
        ExprKind::Array(values) => values.is_empty(),
        ExprKind::Lit(lit) => {
            matches!(lit.node, rustc_ast::LitKind::Str(value, _) if value.as_str().is_empty())
        }
        _ => call(cx, expr).is_some_and(|(id, args)| {
            let path = definition_path(cx, id);
            let name = cx.tcx.item_name(id);
            (collection_method(&path) && name.as_str() == "new" && args.is_empty())
                || (matches!(
                    name.as_str(),
                    "iter" | "into_iter" | "map" | "copied" | "enumerate" | "chars" | "bytes"
                ) && args
                    .first()
                    .is_some_and(|arg| definitely_empty(cx, arg, inputs, seen)))
        }),
    }
}

fn independent<'tcx>(
    cx: &LateContext<'tcx>,
    value: &'tcx Expr<'tcx>,
    fields: &[(Symbol, &'tcx Expr<'tcx>)],
    used: &HashSet<Symbol>,
) -> bool {
    struct Aliases<'a, 'tcx> {
        cx: &'a LateContext<'tcx>,
        bindings: HashSet<HirId>,
    }
    impl<'tcx> Visitor<'tcx> for Aliases<'_, 'tcx> {
        fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
            if let Some(binding) = local_path(self.cx, expr)
                && (matches!(self.cx.tcx.parent_hir_node(binding), Node::LetStmt(_))
                    || (matches!(
                        expr_ty(self.cx, expr).kind(),
                        ty::Adt(..) | ty::Array(..) | ty::Tuple(..)
                    ) && !typeck(self.cx, expr.hir_id)
                        .expr_adjustments(expr)
                        .iter()
                        .any(|adjustment| matches!(adjustment.kind, Adjust::Borrow(_)))))
                && self.bindings.insert(binding)
                && let Some(init) = initializer(self.cx, binding)
            {
                self.visit_expr(init);
            }
            if let ExprKind::Closure(closure) = expr.kind {
                self.visit_expr(self.cx.tcx.hir_body(closure.body).value);
            } else {
                intravisit::walk_expr(self, expr);
            }
        }
    }
    let mut visitor = Aliases {
        cx,
        bindings: HashSet::new(),
    };
    visitor.visit_expr(value);
    let aliases = visitor.bindings;
    // Shared intermediates can remain necessary even through a wrapper or on
    // another return path; discarding their result does not establish waste.
    aliases.is_empty()
        || !fields
            .iter()
            .any(|(name, value)| used.contains(name) && depends_on(cx, value, &aliases))
}

fn collection_inputs<'tcx>(
    cx: &LateContext<'tcx>,
    binding: HirId,
    control: bool,
) -> Vec<&'tcx Expr<'tcx>> {
    let mut inputs = Vec::new();
    if let Node::LetStmt(local) = cx.tcx.parent_hir_node(binding)
        && local.els.is_none()
        && let Some(init) = local.init
    {
        inputs.push(init);
    }
    let Some(body) = cx
        .tcx
        .hir_maybe_body_owned_by(cx.tcx.hir_enclosing_body_owner(binding))
    else {
        return inputs;
    };
    struct Writes<'a, 'tcx> {
        cx: &'a LateContext<'tcx>,
        binding: HirId,
        inputs: Vec<&'tcx Expr<'tcx>>,
        control: bool,
    }
    impl<'tcx> Visitor<'tcx> for Writes<'_, 'tcx> {
        fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
            let writes_binding = match expr.kind {
                ExprKind::Assign(left, _, _) | ExprKind::AssignOp(_, left, _) => {
                    place_binding(self.cx, left) == Some(self.binding)
                }
                _ => call(self.cx, expr).is_some_and(|(id, args)| {
                    collection_method(&definition_path(self.cx, id))
                        && matches!(
                            self.cx.tcx.item_name(id).as_str(),
                            "push" | "push_back" | "push_front" | "push_str" | "insert"
                        )
                        && args
                            .first()
                            .is_some_and(|arg| local_path(self.cx, arg) == Some(self.binding))
                }),
            };
            if writes_binding {
                let mut input = expr;
                // Control-flow inputs can affect a value even when its writes
                // only use constants.
                if self.control {
                    for (_, node) in self.cx.tcx.hir_parent_iter(expr.hir_id) {
                        if let Node::Expr(parent) = node
                            && matches!(parent.kind, ExprKind::Match(..) | ExprKind::If(..))
                        {
                            input = parent;
                        }
                    }
                }
                self.inputs.push(input);
            }
            intravisit::walk_expr(self, expr);
        }
    }
    let mut writes = Writes {
        cx,
        binding,
        inputs,
        control,
    };
    writes.visit_expr(body.value);
    writes.inputs
}

fn result_uses(cx: &LateContext<'_>, expr: &Expr<'_>) -> Option<HashSet<Symbol>> {
    match cx.tcx.parent_hir_node(expr.hir_id) {
        Node::Expr(parent) => match parent.kind {
            ExprKind::Field(base, field)
                if base.hir_id == expr.hir_id && !mutation_context(cx, parent) =>
            {
                Some(HashSet::from([field.name]))
            }
            _ => None,
        },
        Node::LetStmt(binding) if binding.init.is_some_and(|init| init.hir_id == expr.hir_id) => {
            if binding.els.is_some() {
                return None;
            }
            if let Some(id) = simple_binding(binding.pat) {
                binding_uses(cx, id, &mut HashSet::new())
            } else if let PatKind::Struct(_, fields, _) = binding.pat.kind {
                Some(
                    fields
                        .iter()
                        .filter(|field| !matches!(field.pat.kind, PatKind::Wild))
                        .map(|field| field.ident.name)
                        .collect(),
                )
            } else {
                None
            }
        }
        _ => None,
    }
}

fn mutation_context(cx: &LateContext<'_>, expr: &Expr<'_>) -> bool {
    let mut id = expr.hir_id;
    for (_, node) in cx.tcx.hir_parent_iter(id) {
        let Node::Expr(parent) = node else { break };
        match parent.kind {
            ExprKind::Field(base, _) | ExprKind::Index(base, _, _) if base.hir_id == id => {
                id = parent.hir_id
            }
            ExprKind::Assign(left, _, _) | ExprKind::AssignOp(_, left, _) if left.hir_id == id => {
                return true;
            }
            ExprKind::AddrOf(_, rustc_hir::Mutability::Mut, _) => return true,
            ExprKind::MethodCall(..) => {
                let Node::Expr(value) = cx.tcx.hir_node(id) else {
                    return true;
                };
                return typeck(cx, id)
                    .expr_adjustments(value)
                    .iter()
                    .any(|adjustment| {
                        matches!(
                            adjustment.target.kind(),
                            ty::Ref(_, _, rustc_hir::Mutability::Mut)
                        )
                    });
            }
            _ => break,
        }
    }
    false
}

fn binding_uses(
    cx: &LateContext<'_>,
    binding: HirId,
    seen: &mut HashSet<HirId>,
) -> Option<HashSet<Symbol>> {
    if !seen.insert(binding) {
        return None;
    }
    struct Uses<'a, 'tcx> {
        cx: &'a LateContext<'tcx>,
        binding: HirId,
        fields: HashSet<Symbol>,
        aliases: Vec<HirId>,
        escaped: bool,
    }
    impl<'tcx> Visitor<'tcx> for Uses<'_, 'tcx> {
        fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
            if local_path(self.cx, expr) == Some(self.binding) {
                match self.cx.tcx.parent_hir_node(expr.hir_id) {
                    Node::Expr(parent) if matches!(parent.kind, ExprKind::Field(..)) => {
                        let ExprKind::Field(_, field) = parent.kind else {
                            unreachable!()
                        };
                        self.fields.insert(field.name);
                        self.escaped |= mutation_context(self.cx, parent);
                    }
                    Node::LetStmt(binding)
                        if binding.els.is_none() && simple_binding(binding.pat).is_some() =>
                    {
                        self.aliases.push(simple_binding(binding.pat).unwrap())
                    }
                    _ => self.escaped = true,
                }
            }
            if let ExprKind::Closure(closure) = expr.kind {
                let mut nested = Uses {
                    cx: self.cx,
                    binding: self.binding,
                    fields: HashSet::new(),
                    aliases: Vec::new(),
                    escaped: false,
                };
                nested.visit_expr(self.cx.tcx.hir_body(closure.body).value);
                self.escaped |=
                    nested.escaped || !nested.fields.is_empty() || !nested.aliases.is_empty();
            } else {
                intravisit::walk_expr(self, expr);
            }
        }
    }
    let body = cx
        .tcx
        .hir_maybe_body_owned_by(cx.tcx.hir_enclosing_body_owner(binding))?;
    let mut visitor = Uses {
        cx,
        binding,
        fields: HashSet::new(),
        aliases: Vec::new(),
        escaped: false,
    };
    visitor.visit_expr(body.value);
    if visitor.escaped {
        return None;
    }
    for alias in visitor.aliases {
        visitor.fields.extend(binding_uses(cx, alias, seen)?);
    }
    Some(visitor.fields)
}

impl DiscardedProjectionWork {
    fn pure_function<'tcx>(&mut self, cx: &LateContext<'tcx>, id: LocalDefId) -> bool {
        // An overriding implementation need not preserve a default body's effects.
        if matches!(
            cx.tcx.def_kind(cx.tcx.parent(id.to_def_id())),
            rustc_hir::def::DefKind::Trait
        ) {
            return false;
        }
        if let Some(pure) = self.pure.get(&id) {
            return *pure;
        }
        // Recursive calls cannot establish an effect-free summary.
        self.pure.insert(id, false);
        let Some(body) = cx.tcx.hir_maybe_body_owned_by(id) else {
            return false;
        };
        if body.params.iter().any(|param| {
            !safe_drop(
                cx,
                typeck(cx, param.pat.hir_id).node_type(param.pat.hir_id),
                &mut HashSet::new(),
            )
        }) {
            return false;
        }
        let mut visitor = Effects {
            cx,
            lint: self,
            safe: true,
        };
        visitor.visit_expr(body.value);
        let safe = visitor.safe;
        self.pure.insert(id, safe);
        safe
    }
}

struct Effects<'a, 'tcx> {
    cx: &'a LateContext<'tcx>,
    lint: &'a mut DiscardedProjectionWork,
    safe: bool,
}

impl<'tcx> Visitor<'tcx> for Effects<'_, 'tcx> {
    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        if !self.safe {
            return;
        }
        let cx = self.cx;
        if typeck(cx, expr.hir_id)
            .expr_adjustments(expr)
            .iter()
            .any(|adjustment| {
                matches!(
                    adjustment.kind,
                    Adjust::Deref(DerefAdjustKind::Overloaded(_))
                ) && !matches!(
                    collection(cx, expr_ty(cx, expr)),
                    Some(Collection::Vec | Collection::String)
                )
            })
        {
            self.safe = false;
            return;
        }
        match expr.kind {
            ExprKind::Path(ref path) => {
                match typeck(cx, expr.hir_id).qpath_res(path, expr.hir_id) {
                    Res::Def(rustc_hir::def::DefKind::Static { .. }, _) => self.safe = false,
                    Res::Def(rustc_hir::def::DefKind::Ctor(..), _) => {
                        self.safe = safe_drop(cx, expr_ty(cx, expr), &mut HashSet::new())
                    }
                    _ if matches!(expr_ty(cx, expr).kind(), ty::Adt(..)) => {
                        self.safe = safe_drop(cx, expr_ty(cx, expr), &mut HashSet::new());
                    }
                    _ => {}
                }
            }
            ExprKind::Call(..) | ExprKind::MethodCall(..) => {
                self.safe = call(cx, expr).is_some_and(|(id, args)| {
                    if matches!(cx.tcx.def_kind(id), rustc_hir::def::DefKind::Ctor(..)) {
                        safe_drop(cx, expr_ty(cx, expr), &mut HashSet::new())
                    } else if let Some(local) = id.as_local() {
                        self.lint.pure_function(cx, local)
                    } else {
                        standard_call(cx, id, &args, expr)
                    }
                });
                if self.safe
                    && let Some((id, args)) = call(cx, expr)
                {
                    // Standard iterator and Option combinators can invoke arbitrary
                    // callbacks. Only inline closures have bodies checked here.
                    if matches!(
                        cx.tcx.item_name(id).as_str(),
                        "map" | "is_some_and" | "is_none_or"
                    ) {
                        self.safe = args
                            .iter()
                            .skip(1)
                            .all(|arg| matches!(arg.kind, ExprKind::Closure(_)));
                    }
                }
            }
            ExprKind::Assign(left, _, _) => self.safe = owned_place(cx, left),
            ExprKind::AssignOp(_, left, right) => {
                self.safe = owned_place(cx, left)
                    && primitive(expr_ty(cx, left))
                    && primitive(expr_ty(cx, right))
            }
            ExprKind::Struct(..) => {
                self.safe = safe_drop(cx, expr_ty(cx, expr), &mut HashSet::new())
            }
            ExprKind::Binary(_, left, right) => {
                self.safe = primitive(expr_ty(cx, left)) && primitive(expr_ty(cx, right))
            }
            ExprKind::Unary(rustc_hir::UnOp::Deref, value) => {
                self.safe = matches!(expr_ty(cx, value).kind(), ty::Ref(..))
            }
            ExprKind::Unary(_, value) => self.safe = primitive(expr_ty(cx, value)),
            ExprKind::Index(base, _, _) => {
                self.safe = is_vec(cx, expr_ty(cx, base).peel_refs())
                    || matches!(
                        expr_ty(cx, base).peel_refs().kind(),
                        ty::Array(..) | ty::Slice(_)
                    )
            }
            ExprKind::InlineAsm(..) | ExprKind::Yield(..) | ExprKind::ConstBlock(..) => {
                self.safe = false
            }
            ExprKind::Closure(closure) => {
                self.visit_expr(cx.tcx.hir_body(closure.body).value);
                return;
            }
            _ => {}
        }
        if self.safe {
            intravisit::walk_expr(self, expr);
        }
    }
}

fn primitive(ty: Ty<'_>) -> bool {
    matches!(
        ty.peel_refs().kind(),
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_)
    )
}

fn owned_place<'tcx>(cx: &LateContext<'tcx>, expr: &Expr<'tcx>) -> bool {
    match expr.kind {
        ExprKind::Path(_) => {
            local_path(cx, expr).is_some_and(|binding| {
                cx.tcx.hir_enclosing_body_owner(binding)
                    == cx.tcx.hir_enclosing_body_owner(expr.hir_id)
            }) && !matches!(expr_ty(cx, expr).kind(), ty::Ref(..) | ty::RawPtr(..))
                && safe_drop(cx, expr_ty(cx, expr), &mut HashSet::new())
        }
        ExprKind::Field(base, _) | ExprKind::Index(base, _, _) => owned_place(cx, base),
        _ => false,
    }
}

fn place_binding(cx: &LateContext<'_>, expr: &Expr<'_>) -> Option<HirId> {
    match expr.kind {
        ExprKind::Field(base, _) | ExprKind::Index(base, _, _) => place_binding(cx, base),
        _ => local_path(cx, expr),
    }
}

fn standard_call<'tcx>(
    cx: &LateContext<'tcx>,
    id: DefId,
    args: &[&Expr<'tcx>],
    output: &Expr<'tcx>,
) -> bool {
    let path = definition_path(cx, id);
    let name = cx.tcx.item_name(id);
    if !matches!(
        cx.tcx.crate_name(id.krate).as_str(),
        "core" | "alloc" | "std"
    ) {
        return false;
    }
    if path == "core::clone::Clone::clone" {
        return args.first().is_some_and(|arg| {
            let ty = expr_ty(cx, arg).peel_refs();
            let ty::Adt(_, generics) = ty.kind() else {
                return false;
            };
            collection(cx, ty).is_some()
                && generics
                    .types()
                    .take(if collection(cx, ty) == Some(Collection::Map) {
                        2
                    } else {
                        1
                    })
                    .all(trivially_copyable)
        });
    }
    if path == "alloc::borrow::ToOwned::to_owned" {
        return args.first().is_some_and(|arg| {
            matches!(expr_ty(cx, arg).peel_refs().kind(), ty::Str)
                || matches!(expr_ty(cx, arg).peel_refs().kind(), ty::Slice(element) if trivially_copyable(*element))
        });
    }
    if path.starts_with("alloc::slice::") && name.as_str() == "to_vec" {
        return args.first().is_some_and(|arg| {
            matches!(expr_ty(cx, arg).peel_refs().kind(), ty::Slice(element) | ty::Array(element, _) if trivially_copyable(*element))
        });
    }
    if matches!(cx.tcx.def_kind(id), rustc_hir::def::DefKind::Ctor(..)) {
        return safe_drop(cx, expr_ty(cx, output), &mut HashSet::new());
    }
    if name.as_str() == "from" {
        return primitive(expr_ty(cx, output))
            && args.iter().all(|arg| primitive(expr_ty(cx, arg)));
    }
    if path.starts_with("core::ops::range::") && name.as_str() == "new" {
        return standard_iterator(cx, expr_ty(cx, output))
            && args.iter().all(|arg| primitive(expr_ty(cx, arg)));
    }
    if args.first().is_some_and(|arg| primitive(expr_ty(cx, arg))) {
        return matches!(
            name.as_str(),
            "saturating_add"
                | "saturating_sub"
                | "saturating_mul"
                | "min"
                | "max"
                | "div_euclid"
                | "rem_euclid"
                | "unsigned_abs"
                | "abs"
                | "div_ceil"
        );
    }
    if collection_method(&path) {
        return match name.as_str() {
            "new" | "with_capacity" | "len" | "is_empty" => true,
            "push" | "push_back" | "push_front" | "push_str" | "insert" => args.first().is_some_and(|arg| {
                owned_place(cx, arg) && match collection(cx, expr_ty(cx, arg)) {
                    Some(Collection::Map | Collection::Set | Collection::Heap) => {
                        matches!(expr_ty(cx, arg).peel_refs().kind(), ty::Adt(_, generics) if generics.types().next().is_some_and(standard_order))
                    }
                    Some(_) => true,
                    None => false,
                }
            }),
            "to_vec" => args.first().is_some_and(|arg| {
                matches!(expr_ty(cx, arg).peel_refs().kind(), ty::Slice(element) | ty::Array(element, _) if trivially_copyable(*element))
            }),
            "iter" => args.first().is_some_and(|arg| collection(cx, expr_ty(cx, arg)).is_some()),
            "into_iter" => true,
            _ => false,
        };
    }
    if path.starts_with("core::str::") {
        return matches!(name.as_str(), "chars" | "bytes" | "len" | "is_empty");
    }
    if path.starts_with("core::slice::") {
        return matches!(name.as_str(), "iter" | "len" | "is_empty");
    }
    if path.starts_with("core::option::") {
        return matches!(
            name.as_str(),
            "is_some_and" | "is_none_or" | "is_some" | "is_none" | "map"
        );
    }
    if path.starts_with("core::iter::") {
        if name.as_str() == "into_iter" {
            return args.first().is_some_and(|arg| {
                let ty = expr_ty(cx, arg).peel_refs();
                collection(cx, ty).is_some()
                    || matches!(ty.kind(), ty::Slice(_) | ty::Array(..))
                    || standard_iterator(cx, ty)
            });
        }
        // Trait calls on user iterators or caller-supplied callbacks remain unknown.
        return (name.as_str() != "collect" || collect_is_standard(cx, expr_ty(cx, output)))
            && args
                .first()
                .is_some_and(|arg| standard_iterator(cx, expr_ty(cx, arg)))
            && matches!(
                name.as_str(),
                "map" | "enumerate" | "copied" | "collect" | "into_iter" | "next"
            );
    }
    false
}

fn standard_iterator(cx: &LateContext<'_>, ty: Ty<'_>) -> bool {
    match ty.peel_refs().kind() {
        ty::Adt(adt, args) => {
            let path = definition_path(cx, adt.did());
            path.starts_with("core::slice::iter::")
                || matches!(
                    path.as_str(),
                    "core::str::iter::Chars" | "core::str::iter::Bytes"
                )
                || path.starts_with("alloc::vec::into_iter::")
                || path.starts_with("alloc::collections::vec_deque::iter::")
                || path.starts_with("alloc::collections::vec_deque::into_iter::")
                || path == "alloc::collections::linked_list::Iter"
                || (matches!(
                    path.as_str(),
                    "core::ops::range::Range" | "core::ops::range::RangeInclusive"
                ) && args.types().all(primitive))
                || (matches!(
                    path.as_str(),
                    "core::iter::adapters::map::Map"
                        | "core::iter::adapters::enumerate::Enumerate"
                        | "core::iter::adapters::copied::Copied"
                ) && args
                    .types()
                    .next()
                    .is_some_and(|ty| standard_iterator(cx, ty)))
        }
        _ => false,
    }
}

fn collect_is_standard(cx: &LateContext<'_>, ty: Ty<'_>) -> bool {
    match collection(cx, ty) {
        Some(Collection::Map | Collection::Set | Collection::Heap) => {
            matches!(ty.kind(), ty::Adt(_, generics) if generics.types().next().is_some_and(standard_order))
        }
        Some(_) => true,
        None => false,
    }
}

fn standard_order(ty: Ty<'_>) -> bool {
    match ty.peel_refs().kind() {
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Str => true,
        ty::Tuple(types) => types.iter().all(standard_order),
        ty::Array(element, _) | ty::Slice(element) => standard_order(*element),
        _ => false,
    }
}

fn trivially_copyable(ty: Ty<'_>) -> bool {
    match ty.kind() {
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_) | ty::Ref(..) => true,
        ty::Tuple(types) => types.iter().all(trivially_copyable),
        ty::Array(ty, _) => trivially_copyable(*ty),
        // A user Clone implementation may have effects even when all fields are Copy.
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
        direct_field,
        local_binding,
        res_mut,
        system_param,
        free_function,
        ufcs,
        source_alias,
        result_alias,
        destructure,
        nested_field,
        single_component,
        collect_projection,
        local_collection,
        whole_consumer,
        borrowed_consumer,
        retained_checkpoint,
        all_fields,
        collection_used,
        non_ecs_source,
        source_copy,
        result_mutation,
        closure_escape,
        whole_comparison,
        field_borrow,
        query_collection,
        scalar_bundle,
        borrowed_view,
        unknown_effect,
        custom_clone,
        custom_drop,
        element_drop,
        custom_deref,
        unknown_callback,
        custom_collector,
        custom_iterator,
        borrowed_mutation,
        empty_collection,
        source_independent,
        manual_loop,
        multiple_discarded,
        before_after,
        shadowed_binding,
        frogger,
        inclusive_range,
        local_drop,
        custom_assignment,
        ufcs_callback,
        custom_from,
        opaque_projection,
        mutable_field_borrow,
        whole_return,
        sibling_collection,
        renamed_types,
        ignored_source_argument,
        discarded_helper_work,
        retained_field_dependency,
        retained_field_alias,
        copied_projection,
        independent_count,
        retained_helper_dependency,
        explicit_helper_return,
        const_drop,
        assigned_count,
        captured_counter,
        generic_collections,
        owned_string,
        slice_copy,
        nested_materialization,
        tuple_projection,
        early_return,
        early_return_dependency,
        early_return_unknown,
        custom_order,
        empty_input,
        empty_string,
        scalar_computation,
        retained_string_dependency,
        unused_helper_statement,
        zero_sized_elements,
        wrapped_dependency,
        return_control_dependency,
        generic_insertion,
        trait_override,
        assigned_collection_dependency,
        owned_parameter_dependency,
    );
}
