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
    Body, Expr, ExprKind, HirId, Item, ItemKind, Node, TyKind,
    def::{DefKind, Res},
    def_id::{DefId, LocalDefId},
};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::{self, Ty};
use rustc_span::hygiene::{ExpnKind, MacroKind};
use rustc_span::{Span, Symbol};

rustc_session::declare_lint! {
    /// Unknown uses cannot establish the absence of a producer.
    pub BEVY_COMPONENT_WITHOUT_PRODUCER,
    Warn,
    "private Bevy component is queried without a known producer"
}

#[derive(Default)]
struct ComponentWithoutProducer {
    components: HashSet<LocalDefId>,
    blocked: HashSet<LocalDefId>,
    queries: HashMap<LocalDefId, Vec<(Span, Access)>>,
}

#[derive(Clone, Copy)]
enum Access {
    Required,
    Absent,
    Optional,
}

rustc_session::impl_lint_pass!(ComponentWithoutProducer => [BEVY_COMPONENT_WITHOUT_PRODUCER]);
dylint_linting::dylint_library!();

#[unsafe(no_mangle)]
pub fn register_lints(_: &rustc_session::Session, store: &mut rustc_lint::LintStore) {
    store.register_lints(&[BEVY_COMPONENT_WITHOUT_PRODUCER]);
    store.register_late_lint_pass(Box::new(|_| Box::new(ComponentWithoutProducer::default())));
}

impl<'tcx> LateLintPass<'tcx> for ComponentWithoutProducer {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        let ItemKind::Impl(implementation) = item.kind else {
            return;
        };
        let Some(header) = implementation.of_trait else {
            return;
        };
        let Some(trait_id) = header.trait_ref.path.res.opt_def_id() else {
            return;
        };
        if !bevy_definition(cx, trait_id, &["component", "Component"])
            || !component_derive(cx, item.span)
        {
            return;
        }
        let self_ty = cx
            .tcx
            .type_of(item.owner_id)
            .instantiate_identity()
            .skip_norm_wip();
        let ty::Adt(adt, _) = self_ty.kind() else {
            return;
        };
        let Some(id) = adt.did().as_local() else {
            return;
        };
        // Hook expressions can embed arbitrary caller code in the derived impl.
        let custom_component = cx
            .tcx
            .hir_attrs(cx.tcx.local_def_id_to_hir_id(id))
            .iter()
            .any(|attr| attr.has_name(Symbol::intern("component")));
        if cx.tcx.generics_of(id).count() == 0
            && !cx.tcx.effective_visibilities(()).is_reachable(id)
            && !cx.tcx.def_span(id).from_expansion()
            && !custom_component
        {
            self.components.insert(id);
        }
    }

    fn check_ty(
        &mut self,
        cx: &LateContext<'tcx>,
        hir_ty: &'tcx rustc_hir::Ty<'tcx, rustc_hir::AmbigArg>,
    ) {
        let TyKind::Path(ref path) = hir_ty.kind else {
            return;
        };
        let id = match cx.qpath_res(path, hir_ty.hir_id) {
            Res::Def(DefKind::Struct | DefKind::Enum | DefKind::Union, id) => id.as_local(),
            Res::SelfTyAlias { alias_to, .. } => {
                let ty = cx
                    .tcx
                    .type_of(alias_to)
                    .instantiate_identity()
                    .skip_norm_wip();
                match ty.kind() {
                    ty::Adt(adt, _) => adt.did().as_local(),
                    _ => None,
                }
            }
            _ => None,
        };
        let Some(id) = id else { return };
        if inert_derive(cx, hir_ty.hir_id) == Some(id) {
            return;
        }
        if let Some(access) = query_access(cx, hir_ty.hir_id) {
            self.queries
                .entry(id)
                .or_default()
                .push((hir_ty.span, access));
        } else {
            self.blocked.insert(id);
        }
    }

    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let typeck = cx.typeck_results();
        let inert = inert_derive(cx, expr.hir_id);
        let mut owned = HashSet::new();
        collect_types(
            cx,
            typeck.expr_ty(expr),
            true,
            &mut owned,
            &mut HashSet::new(),
        );
        self.blocked
            .extend(owned.into_iter().filter(|id| Some(*id) != inert));

        let (callee, arguments): (_, Vec<_>) = match expr.kind {
            ExprKind::Call(callee, arguments) => {
                let ty::FnDef(id, args) = typeck.expr_ty(callee).kind() else {
                    self.block_type(cx, typeck.expr_ty(callee), inert);
                    for argument in arguments {
                        self.block_type(cx, typeck.expr_ty(argument), inert);
                    }
                    return;
                };
                (Some((*id, args.skip_binder())), arguments.iter().collect())
            }
            ExprKind::MethodCall(_, receiver, arguments, _) => (
                typeck
                    .type_dependent_def_id(expr.hir_id)
                    .map(|id| (id, typeck.node_args(expr.hir_id))),
                std::iter::once(receiver).chain(arguments).collect(),
            ),
            ExprKind::Path(ref path) => {
                // Capturing a constructor as a function value is also a production path.
                if let Res::Def(DefKind::Ctor(..), id) = cx.qpath_res(path, expr.hir_id) {
                    let parent = cx.tcx.parent(id);
                    let parent = if matches!(cx.tcx.def_kind(parent), DefKind::Variant) {
                        cx.tcx.parent(parent)
                    } else {
                        parent
                    };
                    if let Some(id) = parent.as_local()
                        && Some(id) != inert
                    {
                        self.blocked.insert(id);
                    }
                }
                return;
            }
            _ => return,
        };
        let Some((callee, args)) = callee else { return };
        if understood_call(
            cx,
            callee,
            arguments.first().map(|expr| typeck.expr_ty(expr)),
        ) {
            return;
        }
        // Inference can pass a private type to another crate without a written type path.
        for ty in args.types() {
            self.block_type(cx, ty, inert);
        }
        for argument in arguments {
            self.block_type(cx, typeck.expr_ty(argument), inert);
        }
    }

    fn check_body_post(&mut self, cx: &LateContext<'tcx>, body: &Body<'tcx>) {
        // Opaque returns can export a constructor or a system's private query types.
        self.block_type(
            cx,
            cx.typeck_results().expr_ty(body.value),
            inert_derive(cx, body.value.hir_id),
        );
    }

    fn check_crate_post(&mut self, cx: &LateContext<'tcx>) {
        let mut components: Vec<_> = self.components.difference(&self.blocked).copied().collect();
        components.sort_by_key(|id| cx.tcx.def_span(*id).lo());
        for id in components {
            let Some(queries) = self.queries.get(&id) else {
                continue;
            };
            let name = cx.tcx.item_name(id.to_def_id());
            cx.tcx.emit_node_span_lint(
                BEVY_COMPONENT_WITHOUT_PRODUCER,
                cx.tcx.local_def_id_to_hir_id(id),
                cx.tcx.def_span(id),
                DiagDecorator(|diag| {
                    diag.primary_message(format!("component `{name}` is queried, but no producer was found in this crate"));
                    for (span, access) in queries.iter().take(4) {
                        diag.span_note(*span, match access {
                            Access::Required => "this query requires the component and may never match",
                            Access::Absent => "this query excludes a component with no known producer",
                            Access::Optional => "this query has optional or alternative access to a component with no known producer",
                        });
                    }
                    diag.note("this analysis applies to the current build configuration");
                    diag.help("check for missing initialization or obsolete consumers; removing a query constraint can enable previously disabled behavior");
                }),
            );
        }
    }
}

impl ComponentWithoutProducer {
    fn block_type<'tcx>(
        &mut self,
        cx: &LateContext<'tcx>,
        ty: Ty<'tcx>,
        inert: Option<LocalDefId>,
    ) {
        let mut ids = HashSet::new();
        collect_types(cx, ty, false, &mut ids, &mut HashSet::new());
        self.blocked
            .extend(ids.into_iter().filter(|id| Some(*id) != inert));
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
    for (index, name) in path.iter().enumerate() {
        if index + 1 == path.len() {
            return cx.tcx.module_children(current).iter().any(|child| {
                child.ident.name.as_str() == *name && child.res.opt_def_id() == Some(id)
            });
        }
        let Some(child) = cx
            .tcx
            .module_children(current)
            .iter()
            .find(|child| child.ident.name.as_str() == *name)
            .and_then(|child| child.res.opt_def_id())
        else {
            return false;
        };
        current = child;
    }
    current == id
}

fn bevy_definition(cx: &LateContext<'_>, id: DefId, path: &[&str]) -> bool {
    definition(cx, id, "bevy_ecs", path)
}

fn access_type(cx: &LateContext<'_>, id: DefId) -> bool {
    bevy_definition(cx, id, &["system", "Query"])
        || ["Mut", "Ref"]
            .iter()
            .any(|name| bevy_definition(cx, id, &["change_detection", name]))
        || ["With", "Without", "Added", "Changed", "Has", "AnyOf", "Or"]
            .iter()
            .any(|name| bevy_definition(cx, id, &["query", name]))
}

fn component_derive(cx: &LateContext<'_>, span: Span) -> bool {
    let expansion = span.ctxt().outer_expn_data();
    matches!(expansion.kind, ExpnKind::Macro(MacroKind::Derive, name) if name.as_str() == "Component")
        && expansion
            .macro_def_id
            .is_some_and(|id| cx.tcx.crate_name(id.krate).as_str() == "bevy_ecs_macros")
}

fn inert_derive(cx: &LateContext<'_>, id: HirId) -> Option<LocalDefId> {
    for (_, node) in cx.tcx.hir_parent_iter(id) {
        let Node::Item(item) = node else { continue };
        let ItemKind::Impl(implementation) = item.kind else {
            return None;
        };
        let header = implementation.of_trait?;
        let trait_id = header.trait_ref.path.res.opt_def_id()?;
        if !(cx.tcx.is_automatically_derived(item.owner_id.to_def_id())
            || component_derive(cx, item.span))
            || !(bevy_definition(cx, trait_id, &["component", "Component"])
                || [
                    ("clone", "Clone"),
                    ("clone", "TrivialClone"),
                    ("marker", "Copy"),
                    ("marker", "StructuralPartialEq"),
                    ("default", "Default"),
                    ("fmt", "Debug"),
                    ("cmp", "PartialEq"),
                    ("cmp", "Eq"),
                    ("cmp", "PartialOrd"),
                    ("cmp", "Ord"),
                    ("hash", "Hash"),
                ]
                .iter()
                .any(|(module, name)| definition(cx, trait_id, "core", &[module, name])))
        {
            return None;
        }
        let ty = cx
            .tcx
            .type_of(item.owner_id)
            .instantiate_identity()
            .skip_norm_wip();
        return match ty.kind() {
            ty::Adt(adt, _) => adt.did().as_local(),
            _ => None,
        };
    }
    None
}

fn query_access(cx: &LateContext<'_>, id: HirId) -> Option<Access> {
    let mut access = Access::Required;
    for (parent, node) in cx.tcx.hir_parent_iter(id) {
        match node {
            Node::PathSegment(_) | Node::Lifetime(_) => {}
            Node::Ty(hir_ty) => match hir_ty.kind {
                TyKind::Ref(..) | TyKind::Tup(_) => {}
                TyKind::Path(ref path) => {
                    let def = cx.qpath_res(path, parent).opt_def_id()?;
                    if bevy_definition(cx, def, &["system", "Query"]) {
                        return query_position(cx, parent).then_some(access);
                    }
                    if bevy_definition(cx, def, &["query", "Without"]) {
                        access = Access::Absent;
                    } else if bevy_definition(cx, def, &["query", "Has"])
                        || bevy_definition(cx, def, &["query", "AnyOf"])
                        || bevy_definition(cx, def, &["query", "Or"])
                        || definition(cx, def, "core", &["option", "Option"])
                    {
                        access = Access::Optional;
                    } else if !access_type(cx, def) {
                        return None;
                    }
                }
                _ => return None,
            },
            _ => return None,
        }
    }
    None
}

fn query_position(cx: &LateContext<'_>, query: HirId) -> bool {
    match cx.tcx.parent_hir_node(query) {
        Node::Item(item) => match item.kind {
            ItemKind::Fn { sig, .. } => {
                !cx.tcx
                    .effective_visibilities(())
                    .is_reachable(item.owner_id.def_id)
                    && sig.decl.inputs.iter().any(|input| input.hir_id == query)
            }
            _ => false,
        },
        Node::LetStmt(binding) => binding.ty.is_some_and(|ty| ty.hir_id == query),
        _ => false,
    }
}

fn collect_types<'tcx>(
    cx: &LateContext<'tcx>,
    ty: Ty<'tcx>,
    owned: bool,
    ids: &mut HashSet<LocalDefId>,
    seen: &mut HashSet<Ty<'tcx>>,
) {
    if !seen.insert(ty) {
        return;
    }
    match ty.kind() {
        ty::Adt(adt, args) => {
            if owned && access_type(cx, adt.did()) {
                return;
            }
            if let Some(id) = adt.did().as_local() {
                ids.insert(id);
            }
            for ty in args.types() {
                collect_types(cx, ty, owned, ids, seen);
            }
        }
        ty::Ref(_, ty, _) | ty::RawPtr(ty, _) if !owned => collect_types(cx, *ty, owned, ids, seen),
        ty::Array(ty, _) | ty::Slice(ty) => collect_types(cx, *ty, owned, ids, seen),
        ty::Tuple(types) => {
            for ty in types.iter() {
                collect_types(cx, ty, owned, ids, seen);
            }
        }
        ty::FnDef(id, args) if !owned => {
            let args = args.skip_binder();
            for ty in args.types() {
                collect_types(cx, ty, owned, ids, seen);
            }
            let signature = cx
                .tcx
                .fn_sig(*id)
                .instantiate(cx.tcx, args)
                .skip_norm_wip()
                .skip_binder();
            for ty in signature.inputs_and_output.iter() {
                collect_types(cx, ty, owned, ids, seen);
            }
        }
        ty::FnPtr(signature, _) if !owned => {
            for ty in signature.skip_binder().inputs_and_output.iter() {
                collect_types(cx, ty, owned, ids, seen);
            }
        }
        ty::Closure(_, args)
        | ty::CoroutineClosure(_, args)
        | ty::Coroutine(_, args)
        | ty::CoroutineWitness(_, args)
            if !owned =>
        {
            for ty in args.types() {
                collect_types(cx, ty, owned, ids, seen);
            }
        }
        ty::Alias(_, alias) => {
            for ty in alias.args.types() {
                collect_types(cx, ty, owned, ids, seen);
            }
        }
        _ => {}
    }
}

fn understood_call<'tcx>(cx: &LateContext<'tcx>, id: DefId, receiver: Option<Ty<'tcx>>) -> bool {
    let krate = cx.tcx.crate_name(id.krate);
    let name = cx.tcx.item_name(id);
    if krate.as_str() == "bevy_ecs" {
        let parent = cx.tcx.parent(id);
        let trait_id = match cx.tcx.def_kind(parent) {
            DefKind::Trait => Some(parent),
            DefKind::Impl { of_trait: true } => Some(
                cx.tcx
                    .impl_trait_ref(parent)
                    .instantiate_identity()
                    .skip_norm_wip()
                    .def_id,
            ),
            _ => None,
        };
        if trait_id.is_some_and(|id| bevy_definition(cx, id, &["schedule", "IntoScheduleConfigs"]))
        {
            return true;
        }
    }
    if matches!(krate.as_str(), "bevy_app" | "bevy_ecs") && name.as_str() == "add_systems" {
        return cx.tcx.impl_of_assoc(id).is_some_and(|implementation| {
            let ty = cx
                .tcx
                .type_of(implementation)
                .instantiate_identity()
                .skip_norm_wip();
            matches!(ty.kind(), ty::Adt(adt, _) if definition(cx, adt.did(), "bevy_app", &["App"])
                || bevy_definition(cx, adt.did(), &["schedule", "Schedule"]))
        });
    }
    let Some(receiver) = receiver else {
        return false;
    };
    let ty::Adt(adt, _) = receiver.peel_refs().kind() else {
        return false;
    };
    if krate.as_str() == "bevy_ecs" && bevy_definition(cx, adt.did(), &["system", "Query"]) {
        return cx.tcx.impl_of_assoc(id).is_some_and(|implementation| {
            let ty = cx.tcx.type_of(implementation).instantiate_identity().skip_norm_wip();
            matches!(ty.kind(), ty::Adt(adt, _) if bevy_definition(cx, adt.did(), &["system", "Query"]))
        });
    }
    krate.as_str() == "core"
        && matches!(
            name.as_str(),
            "into_iter"
                | "next"
                | "next_back"
                | "nth"
                | "nth_back"
                | "last"
                | "count"
                | "size_hint"
                | "len"
        )
        && ["Query", "QueryIter"].iter().any(|name| {
            bevy_definition(
                cx,
                adt.did(),
                &[if *name == "Query" { "system" } else { "query" }, name],
            )
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

    examples!(
        queries,
        producers,
        required,
        registration,
        macros,
        escapes,
        unknown_uses,
        boundaries
    );
}
