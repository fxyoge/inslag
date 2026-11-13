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
    HirId, Item, ItemKind,
    def_id::{DefId, LocalDefId},
};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty;
use rustc_span::{FileName, RemapPathScopeComponents, Span};

rustc_session::declare_lint! {
    /// Feature-local plugin files keep installation wiring discoverable.
    pub BEVY_STANDARD_PLUGIN_FILENAME,
    Warn,
    "Bevy plugin declarations and implementations belong in plugins.rs"
}

#[derive(Default)]
struct StandardPluginFilename {
    declarations: HashSet<LocalDefId>,
}

rustc_session::impl_lint_pass!(StandardPluginFilename => [BEVY_STANDARD_PLUGIN_FILENAME]);
dylint_linting::dylint_library!();

#[unsafe(no_mangle)]
pub fn register_lints(_: &rustc_session::Session, store: &mut rustc_lint::LintStore) {
    store.register_lints(&[BEVY_STANDARD_PLUGIN_FILENAME]);
    store.register_late_lint_pass(Box::new(|_| Box::new(StandardPluginFilename::default())));
}

impl<'tcx> LateLintPass<'tcx> for StandardPluginFilename {
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
        if !is_plugin_trait(cx, trait_id) {
            return;
        }

        report(cx, item.hir_id(), item.span, "implementation");

        let self_ty = cx
            .tcx
            .type_of(item.owner_id)
            .instantiate_identity()
            .skip_norm_wip();
        if let ty::Adt(adt, _) = self_ty.kind()
            && let Some(id) = adt.did().as_local()
            && self.declarations.insert(id)
        {
            report(
                cx,
                cx.tcx.local_def_id_to_hir_id(id),
                cx.tcx
                    .def_ident_span(id)
                    .unwrap_or_else(|| cx.tcx.def_span(id)),
                "type declaration",
            );
        }
    }
}

fn is_plugin_trait(cx: &LateContext<'_>, id: DefId) -> bool {
    if cx.tcx.crate_name(id.krate).as_str() != "bevy_app" {
        return false;
    }
    let name = cx.tcx.item_name(id);
    matches!(name.as_str(), "Plugin" | "PluginGroup")
        && cx
            .tcx
            .module_children(DefId {
                krate: id.krate,
                index: rustc_hir::def_id::CRATE_DEF_INDEX,
            })
            .iter()
            .any(|child| child.ident.name == name && child.res.opt_def_id() == Some(id))
}

fn report(cx: &LateContext<'_>, id: HirId, span: Span, kind: &str) {
    let span = span.source_callsite();
    let file = cx.sess().source_map().lookup_source_file(span.lo());
    let FileName::Real(filename) = &file.name else {
        return;
    };
    if filename
        .local_path()
        .unwrap_or_else(|| filename.path(RemapPathScopeComponents::DIAGNOSTICS))
        .file_name()
        .is_some_and(|name| name == "plugins.rs")
    {
        return;
    }
    cx.tcx.emit_node_span_lint(
        BEVY_STANDARD_PLUGIN_FILENAME,
        id,
        span,
        DiagDecorator(|diag| {
            diag.primary_message(format!("Bevy plugin {kind} must live in a file named `plugins.rs`"));
            diag.help("move it into the owning feature's `plugins.rs` and re-export the plugin where needed");
        }),
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn filenames() {
        dylint_testing::ui_test_example(env!("CARGO_PKG_NAME"), "filenames");
    }
}
