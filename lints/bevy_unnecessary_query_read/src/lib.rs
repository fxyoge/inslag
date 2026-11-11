#![feature(rustc_private)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

use std::collections::HashSet;

use rustc_errors::DiagDecorator;
use rustc_hir::{
    Body, Expr, ExprKind, GenericArg, HirId, LetStmt, Mutability, Node, PatKind, TyKind,
    def::Res,
    def_id::{DefId, LocalDefId},
    intravisit::{self, Visitor},
};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, Ty, TypeVisitableExt, TypeckResults};
use rustc_span::Span;

rustc_session::declare_lint! {
    /// Unknown query uses or filters cannot establish equivalent membership.
    pub BEVY_UNNECESSARY_QUERY_READ,
    Warn,
    "component read access is used only for Bevy query membership"
}

#[derive(Default)]
struct UnnecessaryQueryRead {
    reports: Vec<Report>,
    contracts: HashSet<LocalDefId>,
}
rustc_session::impl_lint_pass!(UnnecessaryQueryRead => [BEVY_UNNECESSARY_QUERY_READ]);
dylint_linting::dylint_library!();

#[unsafe(no_mangle)]
pub fn register_lints(_: &rustc_session::Session, store: &mut rustc_lint::LintStore) {
    store.register_lints(&[BEVY_UNNECESSARY_QUERY_READ]);
    store.register_late_lint_pass(Box::new(|_| Box::new(UnnecessaryQueryRead::default())));
}

struct Report {
    owner: LocalDefId,
    node: HirId,
    span: Span,
    replacement: String,
}

impl<'tcx> LateLintPass<'tcx> for UnnecessaryQueryRead {
    fn check_body(&mut self, cx: &LateContext<'tcx>, body: &Body<'tcx>) {
        let owner = cx.tcx.hir_body_owner_def_id(body.id());
        if cx.tcx.opt_associated_item(owner.to_def_id()).is_some()
            || cx.tcx.effective_visibilities(()).is_reachable(owner)
        {
            return;
        }
        let Some(declaration) = cx
            .tcx
            .hir_fn_decl_by_hir_id(cx.tcx.local_def_id_to_hir_id(owner))
        else {
            return;
        };
        for (param, input) in body.params.iter().zip(declaration.inputs) {
            let TyKind::Path(rustc_hir::QPath::Resolved(_, path)) = input.kind else {
                continue;
            };
            if input.span.from_expansion()
                || !path
                    .res
                    .opt_def_id()
                    .is_some_and(|id| definition(cx, id, "bevy_ecs", &["system", "Query"]))
            {
                continue;
            }
            let PatKind::Binding(_, binding, _, None) = param.pat.kind else {
                continue;
            };
            let ty::Adt(_, args) = typeck(cx, param.hir_id).node_type(param.pat.hir_id).kind()
            else {
                continue;
            };
            let Some(generics) = path.segments.last().and_then(|segment| segment.args) else {
                continue;
            };
            let types: Vec<_> = generics
                .args
                .iter()
                .filter_map(|arg| match arg {
                    GenericArg::Type(ty) => Some(*ty),
                    _ => None,
                })
                .collect();
            let Some(data) = types.first() else {
                continue;
            };
            let mut components = Vec::new();
            if !components_in(cx, data, args.type_at(2), &mut components) || components.is_empty() {
                continue;
            }
            let mut reads = Vec::new();
            if !filter_reads(cx, args.type_at(3), &mut reads)
                || components.iter().all(|(ty, _)| reads.contains(ty))
            {
                continue;
            }
            // Distinct generic components can coincide after monomorphization.
            if !reads.is_empty()
                && components
                    .iter()
                    .map(|(ty, _)| ty)
                    .chain(&reads)
                    .any(|ty| ty.has_non_region_param() || ty.has_aliases())
            {
                continue;
            }
            let mut analysis = Analysis {
                cx,
                bindings: HashSet::from([binding]),
                safe: true,
                observed: false,
            };
            analysis.visit_expr(body.value);
            if !analysis.safe || !analysis.observed {
                continue;
            }
            let mut filters: Vec<_> = components
                .iter()
                .map(|(_, component)| format!("With<{component}>"))
                .collect();
            if let Some(filter) = types.get(1) {
                let Ok(filter) = cx.sess().source_map().span_to_snippet(filter.span) else {
                    continue;
                };
                filters.push(filter);
            }
            // Nested query data can exceed Bevy's flat filter tuple arity limit.
            let Some(filter) = filters
                .into_iter()
                .reduce(|left, right| format!("({left}, {right})"))
            else {
                continue;
            };
            let lifetimes: Option<Vec<_>> = generics
                .args
                .iter()
                .filter_map(|arg| match arg {
                    GenericArg::Lifetime(lifetime) => Some(
                        cx.sess()
                            .source_map()
                            .span_to_snippet(lifetime.ident.span)
                            .ok(),
                    ),
                    _ => None,
                })
                .collect();
            let Some(mut arguments) = lifetimes else {
                continue;
            };
            arguments.retain(|argument| !argument.is_empty());
            arguments.extend(["()".into(), filter]);
            self.reports.push(Report {
                owner,
                node: param.hir_id,
                span: data.span,
                replacement: format!(
                    "{}<{}>",
                    path.segments.last().unwrap().ident,
                    arguments.join(", ")
                ),
            });
        }
    }

    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let id = match (expr.kind, expr_ty(cx, expr).kind()) {
            (ExprKind::Path(_), ty::FnDef(id, _) | ty::Closure(id, _)) => *id,
            (ExprKind::Closure(_), ty::Closure(id, _)) => {
                if matches!(cx.tcx.parent_hir_node(expr.hir_id), Node::LetStmt(local) if local.ty.is_none())
                {
                    return;
                }
                *id
            }
            _ => return,
        };
        if let Some(id) = id.as_local()
            && !system_use(cx, expr)
        {
            // Helpers and coercions can require the original concrete signature.
            self.contracts.insert(id);
        }
    }

    fn check_crate_post(&mut self, cx: &LateContext<'tcx>) {
        self.reports.sort_by_key(|report| report.span.lo());
        for report in &self.reports {
            if self.contracts.contains(&report.owner) {
                continue;
            }
            cx.tcx.emit_node_span_lint(
                BEVY_UNNECESSARY_QUERY_READ,
                report.node,
                report.span,
                DiagDecorator(|diag| {
                    diag.primary_message(
                        "query requests component read access, but only membership is used",
                    );
                    diag.help(format!(
                        "use `{}` to avoid unnecessary scheduling conflicts",
                        report.replacement
                    ));
                }),
            );
        }
    }
}

fn components_in<'tcx, A>(
    cx: &LateContext<'tcx>,
    source: &rustc_hir::Ty<'tcx, A>,
    ty: Ty<'tcx>,
    components: &mut Vec<(Ty<'tcx>, String)>,
) -> bool {
    match (&source.kind, ty.kind()) {
        (TyKind::Ref(_, reference), ty::Ref(_, component, Mutability::Not)) => {
            let Ok(name) = cx.sess().source_map().span_to_snippet(reference.ty.span) else {
                return false;
            };
            if !components.iter().any(|(ty, _)| ty == component) {
                components.push((*component, name));
            }
            true
        }
        (TyKind::Tup(sources), ty::Tuple(fields)) if sources.len() == fields.len() => sources
            .iter()
            .zip(fields.iter())
            .all(|(source, ty)| components_in(cx, source, ty, components)),
        (_, ty::Adt(adt, _)) => definition(cx, adt.did(), "bevy_ecs", &["entity", "Entity"]),
        _ => false,
    }
}

fn filter_reads<'tcx>(cx: &LateContext<'tcx>, ty: Ty<'tcx>, reads: &mut Vec<Ty<'tcx>>) -> bool {
    match ty.kind() {
        ty::Tuple(filters) => filters.iter().all(|ty| filter_reads(cx, ty, reads)),
        ty::Adt(adt, args) => {
            let id = adt.did();
            if ["With", "Without"]
                .iter()
                .any(|name| definition(cx, id, "bevy_ecs", &["query", name]))
            {
                true
            } else if ["Added", "Changed"]
                .iter()
                .any(|name| definition(cx, id, "bevy_ecs", &["query", name]))
            {
                reads.push(args.type_at(0));
                true
            } else if definition(cx, id, "bevy_ecs", &["query", "Or"]) {
                filter_reads(cx, args.type_at(0), reads)
            } else {
                false
            }
        }
        _ => false,
    }
}

struct Analysis<'a, 'tcx> {
    cx: &'a LateContext<'tcx>,
    bindings: HashSet<HirId>,
    safe: bool,
    observed: bool,
}

impl<'tcx> Analysis<'_, 'tcx> {
    fn tracked(&self, expr: &Expr<'tcx>) -> bool {
        match expr.kind {
            ExprKind::Path(ref path) => {
                matches!(typeck(self.cx, expr.hir_id).qpath_res(path, expr.hir_id), Res::Local(id) if self.bindings.contains(&id))
            }
            ExprKind::AddrOf(_, Mutability::Not, inner) | ExprKind::DropTemps(inner) => {
                self.tracked(inner)
            }
            _ => false,
        }
    }

    fn membership(&self, expr: &Expr<'tcx>) -> bool {
        if expr.span.from_expansion() || explicit_call_types(expr) {
            return false;
        }
        call(self.cx, expr).is_some_and(|(id, receiver)| {
            self.tracked(receiver)
                && inherent_query(self.cx, id)
                && matches!(self.cx.tcx.item_name(id).as_str(), "contains" | "is_empty")
        })
    }

    fn allowed(&self, expr: &Expr<'tcx>) -> bool {
        if expr.span.from_expansion() {
            return false;
        }
        match self.cx.tcx.parent_hir_node(expr.hir_id) {
            Node::LetStmt(local) => {
                local.ty.is_none()
                    && matches!(local.pat.kind, PatKind::Binding(_, _, _, None))
                    && local.init.is_some_and(|init| init.hir_id == expr.hir_id)
            }
            Node::Expr(parent) => match parent.kind {
                ExprKind::AddrOf(_, Mutability::Not, _) | ExprKind::DropTemps(_) => true,
                ExprKind::Call(..) | ExprKind::MethodCall(..) => {
                    self.membership(parent)
                        && call(self.cx, parent)
                            .is_some_and(|(_, receiver)| receiver.hir_id == expr.hir_id)
                }
                _ => false,
            },
            _ => false,
        }
    }
}

impl<'tcx> Visitor<'tcx> for Analysis<'_, 'tcx> {
    fn visit_local(&mut self, local: &'tcx LetStmt<'tcx>) {
        if let Some(init) = local.init {
            self.visit_expr(init);
            if self.tracked(init)
                && let PatKind::Binding(_, binding, _, None) = local.pat.kind
            {
                self.bindings.insert(binding);
            }
        }
        if let Some(otherwise) = local.els {
            self.visit_block(otherwise);
        }
    }

    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        if !self.safe {
            return;
        }
        if let ExprKind::Closure(closure) = expr.kind {
            // Even a shared capture can escape into a concrete Query signature.
            let mut captures = Captures {
                cx: self.cx,
                bindings: &self.bindings,
                found: false,
            };
            captures.visit_body(self.cx.tcx.hir_body(closure.body));
            self.safe &= !captures.found;
            return;
        }
        if self.tracked(expr) {
            self.safe &= self.allowed(expr);
        }
        self.observed |= self.membership(expr);
        intravisit::walk_expr(self, expr);
    }
}

fn explicit_call_types(expr: &Expr<'_>) -> bool {
    if let ExprKind::MethodCall(segment, ..) = expr.kind {
        return segment.args.is_some_and(|args| {
            args.args
                .iter()
                .any(|arg| matches!(arg, GenericArg::Type(_)))
        });
    }
    let ExprKind::Call(callee, _) = expr.kind else {
        return false;
    };
    let ExprKind::Path(ref path) = callee.kind else {
        return true;
    };
    let path = match path {
        rustc_hir::QPath::Resolved(_, path) => *path,
        rustc_hir::QPath::TypeRelative(ty, _) => {
            let TyKind::Path(rustc_hir::QPath::Resolved(_, path)) = ty.kind else {
                return true;
            };
            path
        }
    };
    // Explicit UFCS types can impose the same contract as a typed helper call.
    path.segments.iter().any(|segment| {
        segment.args.is_some_and(|args| {
            args.args
                .iter()
                .any(|arg| matches!(arg, GenericArg::Type(_)))
        })
    })
}

struct Captures<'a, 'tcx> {
    cx: &'a LateContext<'tcx>,
    bindings: &'a HashSet<HirId>,
    found: bool,
}

impl<'tcx> Visitor<'tcx> for Captures<'_, 'tcx> {
    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        if let ExprKind::Path(ref path) = expr.kind
            && let Res::Local(id) = typeck(self.cx, expr.hir_id).qpath_res(path, expr.hir_id)
        {
            self.found |= self.bindings.contains(&id);
        }
        if let ExprKind::Closure(closure) = expr.kind {
            self.visit_body(self.cx.tcx.hir_body(closure.body));
        } else {
            intravisit::walk_expr(self, expr);
        }
    }
}

fn typeck<'tcx>(cx: &LateContext<'tcx>, id: HirId) -> &'tcx TypeckResults<'tcx> {
    cx.tcx.typeck(cx.tcx.hir_enclosing_body_owner(id))
}

fn expr_ty<'tcx>(cx: &LateContext<'tcx>, expr: &Expr<'tcx>) -> Ty<'tcx> {
    typeck(cx, expr.hir_id).expr_ty(expr)
}

fn call<'tcx>(cx: &LateContext<'tcx>, expr: &Expr<'tcx>) -> Option<(DefId, &'tcx Expr<'tcx>)> {
    match expr.kind {
        ExprKind::MethodCall(_, receiver, _, _) => Some((
            typeck(cx, expr.hir_id).type_dependent_def_id(expr.hir_id)?,
            receiver,
        )),
        ExprKind::Call(callee, [receiver, ..]) => match expr_ty(cx, callee).kind() {
            ty::FnDef(id, _) => Some((*id, receiver)),
            _ => None,
        },
        _ => None,
    }
}

fn definition(cx: &LateContext<'_>, id: DefId, krate: &str, path: &[&str]) -> bool {
    if cx.tcx.crate_name(id.krate).as_str() != krate {
        return false;
    }
    let mut current = DefId {
        krate: id.krate,
        index: rustc_hir::def_id::CRATE_DEF_INDEX,
    };
    for name in path {
        let Some(next) = cx
            .tcx
            .module_children(current)
            .iter()
            .find(|child| child.ident.name.as_str() == *name)
            .and_then(|child| child.res.opt_def_id())
        else {
            return false;
        };
        current = next;
    }
    current == id
}

fn inherent_query(cx: &LateContext<'_>, id: DefId) -> bool {
    cx.tcx.impl_of_assoc(id).is_some_and(|implementation| {
        !cx.tcx.impl_is_of_trait(implementation)
            && matches!(cx.tcx.type_of(implementation).instantiate_identity().skip_norm_wip().kind(),
                ty::Adt(adt, _) if definition(cx, adt.did(), "bevy_ecs", &["system", "Query"]))
    })
}

fn system_use<'tcx>(cx: &LateContext<'tcx>, expr: &Expr<'tcx>) -> bool {
    let mut current = expr.hir_id;
    while let Node::Expr(parent) = cx.tcx.parent_hir_node(current) {
        if matches!(parent.kind, ExprKind::Tup(_)) {
            current = parent.hir_id;
            continue;
        }
        return !explicit_call_types(parent)
            && call(cx, parent).is_some_and(|(id, _)| {
                matches!(
                    cx.tcx.crate_name(id.krate).as_str(),
                    "bevy_app" | "bevy_ecs"
                ) && matches!(
                    cx.tcx.item_name(id).as_str(),
                    "add_systems"
                        | "add_observer"
                        | "run_if"
                        | "distributive_run_if"
                        | "before"
                        | "after"
                        | "in_set"
                        | "chain"
                        | "chain_ignore_deferred"
                        | "into_configs"
                        | "register_system"
                        | "run_system_cached"
                )
            });
    }
    false
}

#[cfg(test)]
mod tests;
