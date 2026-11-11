#![feature(rustc_private)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

use std::collections::{HashMap, HashSet};

use rustc_errors::DiagDecorator;
use rustc_hir::{
    Body, Expr, ExprKind, GenericArg, HirId, LetStmt, Mutability, Node, Pat, PatKind, TyKind,
    def::Res,
    def_id::{DefId, LocalDefId},
    intravisit::{self, Visitor},
};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::adjustment::{Adjust, DerefAdjustKind};
use rustc_middle::ty::{self, Ty, TypeckResults};
use rustc_span::{Span, hygiene::ExpnKind};

rustc_session::declare_lint! {
    /// Unknown uses cannot establish that shared access is sufficient.
    pub BEVY_UNNECESSARY_MUT_QUERY,
    Warn,
    "mutable Bevy query field is used only through shared access"
}

#[derive(Default)]
struct UnnecessaryMutQuery {
    reports: Vec<Report>,
    contracts: HashSet<LocalDefId>,
}
rustc_session::impl_lint_pass!(UnnecessaryMutQuery => [BEVY_UNNECESSARY_MUT_QUERY]);
dylint_linting::dylint_library!();

#[unsafe(no_mangle)]
pub fn register_lints(_: &rustc_session::Session, store: &mut rustc_lint::LintStore) {
    store.register_lints(&[BEVY_UNNECESSARY_MUT_QUERY]);
    store.register_late_lint_pass(Box::new(|_| Box::new(UnnecessaryMutQuery::default())));
}

struct Report {
    owner: LocalDefId,
    node: HirId,
    span: Span,
    component: String,
}

impl<'tcx> LateLintPass<'tcx> for UnnecessaryMutQuery {
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
            let TyKind::Path(ref path) = input.kind else {
                continue;
            };
            if !cx
                .qpath_res(path, input.hir_id)
                .opt_def_id()
                .is_some_and(|id| definition(cx, id, "bevy_ecs", &["system", "Query"]))
                || input.span.from_expansion()
            {
                continue;
            }
            let rustc_hir::QPath::Resolved(_, path) = path else {
                continue;
            };
            let Some(data) = path
                .segments
                .last()
                .and_then(|segment| segment.args)
                .and_then(|args| {
                    args.args.iter().find_map(|arg| match arg {
                        GenericArg::Type(ty) => Some(*ty),
                        _ => None,
                    })
                })
            else {
                continue;
            };
            for candidate in candidates(data) {
                let mut analysis = Analysis {
                    cx,
                    bindings: HashMap::new(),
                    safe: true,
                    observed: false,
                };
                analysis.bind(param.pat, &Origin::Query(Box::new(candidate.origin)));
                analysis.visit_expr(body.value);
                if !analysis.safe || !analysis.observed {
                    continue;
                }
                let Ok(component) = cx.sess().source_map().span_to_snippet(candidate.component)
                else {
                    continue;
                };
                self.reports.push(Report {
                    owner,
                    node: param.hir_id,
                    span: candidate.span,
                    component,
                });
            }
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
            // Helpers and function-pointer contracts can require this exact signature.
            self.contracts.insert(id);
        }
    }

    fn check_crate_post(&mut self, cx: &LateContext<'tcx>) {
        self.reports.sort_by_key(|report| report.span.lo());
        for report in &self.reports {
            if self.contracts.contains(&report.owner) {
                continue;
            }
            let component = &report.component;
            cx.tcx.emit_node_span_lint(
                BEVY_UNNECESSARY_MUT_QUERY,
                report.node,
                report.span,
                DiagDecorator(|diag| {
                    diag.primary_message(format!(
                        "query requests mutable access to `{component}`, but only shared access is used"
                    ));
                    diag.help(format!(
                        "use `&{component}` to avoid unnecessary scheduling conflicts"
                    ));
                }),
            );
        }
    }
}

fn system_use<'tcx>(cx: &LateContext<'tcx>, expr: &Expr<'tcx>) -> bool {
    let mut current = expr.hir_id;
    while let Node::Expr(parent) = cx.tcx.parent_hir_node(current) {
        if matches!(parent.kind, ExprKind::Tup(_)) {
            current = parent.hir_id;
            continue;
        }
        return call(cx, parent).is_some_and(|(id, _)| {
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

#[derive(Clone)]
enum Origin {
    None,
    Item,
    Query(Box<Self>),
    Iterator(Box<Self>),
    Tuple(Vec<Self>),
    Option(Box<Self>),
    Result(Box<Self>),
}

impl Origin {
    fn tracked(&self) -> bool {
        !matches!(self, Self::None)
    }
}

struct Candidate {
    span: Span,
    component: Span,
    origin: Origin,
}

fn candidates<A>(data: &rustc_hir::Ty<'_, A>) -> Vec<Candidate> {
    match data.kind {
        TyKind::Ref(_, reference) if reference.mutbl == Mutability::Mut => vec![Candidate {
            span: data.span,
            component: reference.ty.span,
            origin: Origin::Item,
        }],
        TyKind::Tup(fields) => fields
            .iter()
            .enumerate()
            .flat_map(|(index, field)| {
                candidates(field).into_iter().map(move |mut candidate| {
                    let mut fields = vec![Origin::None; fields.len()];
                    fields[index] = candidate.origin;
                    candidate.origin = Origin::Tuple(fields);
                    candidate
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

struct Analysis<'a, 'tcx> {
    cx: &'a LateContext<'tcx>,
    bindings: HashMap<HirId, Origin>,
    safe: bool,
    observed: bool,
}

impl<'tcx> Analysis<'_, 'tcx> {
    fn bind(&mut self, pat: &Pat<'tcx>, origin: &Origin) {
        if !origin.tracked() {
            return;
        }
        match (&pat.kind, origin) {
            (PatKind::Wild, _) => {}
            (PatKind::Binding(_, id, _, sub), _) => {
                self.bindings.insert(*id, origin.clone());
                if let Some(sub) = sub {
                    self.bind(sub, origin);
                }
            }
            (PatKind::Tuple(pats, rest), Origin::Tuple(fields)) => {
                for (index, pat) in pats.iter().enumerate() {
                    let index = if rest.as_opt_usize().is_some_and(|rest| index >= rest) {
                        index + fields.len() - pats.len()
                    } else {
                        index
                    };
                    if let Some(field) = fields.get(index) {
                        self.bind(pat, field);
                    } else {
                        self.safe = false;
                    }
                }
            }
            (
                PatKind::TupleStruct(path, pats, _),
                Origin::Option(inner) | Origin::Result(inner),
            ) => {
                let name = variant_name(self.cx, pat, path);
                if name.is_some_and(|name| matches!(name.as_str(), "Some" | "Ok"))
                    && pats.len() == 1
                {
                    self.bind(&pats[0], inner);
                }
            }
            (PatKind::Struct(path, fields, _), Origin::Option(inner) | Origin::Result(inner)) => {
                if variant_name(self.cx, pat, path)
                    .is_some_and(|name| matches!(name.as_str(), "Some" | "Ok"))
                {
                    for field in *fields {
                        if field.ident.name.as_str() == "0" {
                            self.bind(field.pat, inner);
                        } else {
                            self.safe = false;
                        }
                    }
                }
            }
            (PatKind::Expr(_), Origin::Option(_) | Origin::Result(_)) => {}
            (PatKind::Or(pats), _) => {
                for pat in *pats {
                    self.bind(pat, origin);
                }
            }
            _ => self.safe = false,
        }
    }

    fn origin(&self, expr: &Expr<'tcx>) -> Origin {
        let origin = match expr.kind {
            ExprKind::Path(ref path) => {
                match typeck(self.cx, expr.hir_id).qpath_res(path, expr.hir_id) {
                    Res::Local(id) => self.bindings.get(&id).cloned().unwrap_or(Origin::None),
                    _ => Origin::None,
                }
            }
            ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) => self.origin(inner),
            ExprKind::Field(base, field) => match self.origin(base) {
                Origin::Tuple(fields) => field
                    .name
                    .as_str()
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| fields.get(index).cloned())
                    .unwrap_or(Origin::None),
                _ => Origin::None,
            },
            ExprKind::Call(..) | ExprKind::MethodCall(..) => {
                self.operation(expr).unwrap_or(Origin::None)
            }
            ExprKind::Block(block, _) => block.expr.map_or(Origin::None, |expr| self.origin(expr)),
            _ => Origin::None,
        };
        if matches!(origin, Origin::Item) && shared_deref(self.cx, expr) {
            Origin::None
        } else {
            origin
        }
    }

    fn operation(&self, expr: &Expr<'tcx>) -> Option<Origin> {
        let (id, receiver) = call(self.cx, expr)?;
        let origin = self.origin(receiver);
        let name = self.cx.tcx.item_name(id);
        match origin {
            Origin::Query(item) if inherent(self.cx, id, "bevy_ecs", &["system", "Query"]) => {
                match name.as_str() {
                    "iter_mut" => Some(Origin::Iterator(item)),
                    "get_mut" | "single_mut" => Some(Origin::Result(item)),
                    "iter" | "get" | "single" | "contains" | "is_empty" => Some(Origin::None),
                    _ => None,
                }
            }
            Origin::Query(item)
                if core_trait(self.cx, id, &["iter", "IntoIterator"])
                    && name.as_str() == "into_iter" =>
            {
                let mutable = !matches!(
                    typeck(self.cx, receiver.hir_id)
                        .expr_ty_adjusted(receiver)
                        .kind(),
                    ty::Ref(_, _, Mutability::Not)
                );
                Some(if mutable {
                    Origin::Iterator(item)
                } else {
                    Origin::None
                })
            }
            Origin::Iterator(item)
                if core_trait(self.cx, id, &["iter", "Iterator"]) && name.as_str() == "next" =>
            {
                Some(Origin::Option(item))
            }
            Origin::Iterator(item)
                if core_trait(self.cx, id, &["iter", "IntoIterator"])
                    && name.as_str() == "into_iter" =>
            {
                Some(Origin::Iterator(item))
            }
            Origin::Option(item) if inherent(self.cx, id, "core", &["option", "Option"]) => {
                match name.as_str() {
                    "unwrap" | "expect" => Some(*item),
                    "is_some" | "is_none" => Some(Origin::None),
                    _ => None,
                }
            }
            Origin::Result(item) if inherent(self.cx, id, "core", &["result", "Result"]) => {
                match name.as_str() {
                    "unwrap" | "expect" => Some(*item),
                    "is_ok" | "is_err" => Some(Origin::None),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn allowed(&self, expr: &Expr<'tcx>, origin: &Origin) -> bool {
        if user_macro(expr.span) {
            return false;
        }
        match self.cx.tcx.parent_hir_node(expr.hir_id) {
            Node::LetStmt(local) => {
                local.ty.is_none() && local.init.is_some_and(|init| init.hir_id == expr.hir_id)
            }
            Node::Expr(parent) => match parent.kind {
                ExprKind::AddrOf(_, mutability, _) => {
                    !matches!(origin, Origin::Item) || mutability == Mutability::Not
                }
                ExprKind::DropTemps(_) => true,
                ExprKind::Field(..) => matches!(origin, Origin::Tuple(_)),
                ExprKind::Match(scrutinee, _, _) => scrutinee.hir_id == expr.hir_id,
                ExprKind::Let(binding) => binding.init.hir_id == expr.hir_id,
                ExprKind::Call(..) | ExprKind::MethodCall(..) => {
                    call(self.cx, parent)
                        .is_some_and(|(_, receiver)| receiver.hir_id == expr.hir_id)
                        && self.operation(parent).is_some()
                }
                ExprKind::Unary(rustc_hir::UnOp::Deref, _) if matches!(origin, Origin::Item) => {
                    shared_explicit_deref(self.cx, parent)
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
            self.bind(local.pat, &self.origin(init));
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
            // An unaccounted capture can export the mutable capability.
            let body = self.cx.tcx.hir_body(closure.body);
            let captured = self.bindings.keys().copied().collect::<Vec<_>>();
            self.safe &= !captures(self.cx, body, &captured);
            return;
        }
        let origin = self.origin(expr);
        if origin.tracked() {
            self.safe &= self.allowed(expr, &origin);
        }
        if let ExprKind::Path(ref path) = expr.kind
            && let Res::Local(id) = typeck(self.cx, expr.hir_id).qpath_res(path, expr.hir_id)
            && self
                .bindings
                .get(&id)
                .is_some_and(|origin| matches!(origin, Origin::Item))
            && mutable_deref(self.cx, expr)
        {
            self.safe = false;
        }
        if let Some((_, receiver)) = call(self.cx, expr)
            && self.origin(receiver).tracked()
            && self.operation(expr).is_some()
        {
            self.observed = true;
        }
        if let ExprKind::Match(scrutinee, arms, _) = expr.kind {
            self.visit_expr(scrutinee);
            let origin = self.origin(scrutinee);
            for arm in arms {
                self.bind(arm.pat, &origin);
                self.visit_arm(arm);
            }
        } else if let ExprKind::Let(binding) = expr.kind {
            self.visit_expr(binding.init);
            self.bind(binding.pat, &self.origin(binding.init));
        } else {
            intravisit::walk_expr(self, expr);
        }
    }
}

fn typeck<'tcx>(cx: &LateContext<'tcx>, id: HirId) -> &'tcx TypeckResults<'tcx> {
    cx.tcx.typeck(cx.tcx.hir_enclosing_body_owner(id))
}

fn variant_name(
    cx: &LateContext<'_>,
    pat: &Pat<'_>,
    path: &rustc_hir::QPath<'_>,
) -> Option<rustc_span::Symbol> {
    let id = typeck(cx, pat.hir_id)
        .qpath_res(path, pat.hir_id)
        .opt_def_id()?;
    let variant = if matches!(cx.tcx.def_kind(id), rustc_hir::def::DefKind::Ctor(..)) {
        cx.tcx.parent(id)
    } else {
        id
    };
    Some(cx.tcx.item_name(variant))
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

fn mutable_deref(cx: &LateContext<'_>, expr: &Expr<'_>) -> bool {
    typeck(cx, expr.hir_id).expr_adjustments(expr).iter().any(|adjustment| {
        matches!(adjustment.kind, Adjust::Deref(DerefAdjustKind::Overloaded(deref)) if deref.mutbl == Mutability::Mut)
    })
}

fn shared_deref<'tcx>(cx: &LateContext<'tcx>, expr: &Expr<'tcx>) -> bool {
    let ty = expr_ty(cx, expr).peel_refs();
    matches!(ty.kind(), ty::Adt(adt, _) if definition(cx, adt.did(), "bevy_ecs", &["change_detection", "Mut"]))
        && !mutable_deref(cx, expr)
        && typeck(cx, expr.hir_id).expr_adjustments(expr).iter().any(|adjustment| {
            matches!(adjustment.kind, Adjust::Deref(DerefAdjustKind::Overloaded(deref)) if deref.mutbl == Mutability::Not)
        })
}

fn shared_explicit_deref(cx: &LateContext<'_>, expr: &Expr<'_>) -> bool {
    typeck(cx, expr.hir_id)
        .type_dependent_def_id(expr.hir_id)
        .is_some_and(|id| core_trait(cx, id, &["ops", "Deref"]))
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

fn inherent(cx: &LateContext<'_>, id: DefId, krate: &str, path: &[&str]) -> bool {
    cx.tcx.impl_of_assoc(id).is_some_and(|implementation| {
        !cx.tcx.impl_is_of_trait(implementation)
            && matches!(cx.tcx.type_of(implementation).instantiate_identity().skip_norm_wip().kind(),
                ty::Adt(adt, _) if definition(cx, adt.did(), krate, path))
    })
}

fn core_trait(cx: &LateContext<'_>, id: DefId, path: &[&str]) -> bool {
    let parent = cx.tcx.parent(id);
    let trait_id = cx
        .tcx
        .opt_associated_item(id)
        .and_then(|item| item.trait_item_def_id())
        .map_or(parent, |item| cx.tcx.parent(item));
    definition(cx, trait_id, "core", path)
}

fn user_macro(span: Span) -> bool {
    span.from_expansion() && matches!(span.ctxt().outer_expn_data().kind, ExpnKind::Macro(..))
}

fn captures<'tcx>(cx: &LateContext<'tcx>, body: &Body<'tcx>, bindings: &[HirId]) -> bool {
    struct Captures<'a, 'tcx> {
        cx: &'a LateContext<'tcx>,
        bindings: &'a [HirId],
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
    let mut visitor = Captures {
        cx,
        bindings,
        found: false,
    };
    visitor.visit_body(body);
    visitor.found
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

    examples!(
        mixed_query,
        read_only_iter,
        read_only_get,
        read_only_single,
        iter_mut_read,
        get_mut_read,
        single_mut_read,
        let_else_read,
        match_read,
        tuple_projection_read,
        local_alias_read,
        shared_helper,
        nested_tuple_read,
        renamed_query,
        closure_system,
        tuple_rest_read,
        field_assignment,
        compound_assignment,
        mutable_method,
        mutable_helper,
        wrapper_helper,
        borrowed_wrapper_helper,
        query_helper,
        iterator_escape,
        closure_capture,
        branch_mutation,
        alias_mutation,
        set_changed,
        bypass_change_detection,
        set_if_neq,
        into_inner,
        map_unchanged,
        change_metadata,
        annotated_binding,
        unsupported_adapter,
        query_lens,
        custom_query_data,
        query_alias,
        param_set,
        system_param,
        unrelated_query,
        public_api,
        trait_method,
        raw_pointer,
        macro_escape,
        owned_iterator_mutation,
        function_pointer_contract,
        direct_helper_contract,
        closure_signature_contract,
        registered_system,
        multiple_fetch_sites,
        tuple_field_mutation,
        explicit_deref_mutation,
        shared_method,
        filter_preserved,
        allowed_query,
        expected_query,
        inferred_query_alias,
    );
}
