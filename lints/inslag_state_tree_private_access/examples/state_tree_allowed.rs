#![allow(dead_code)]
#![deny(unfulfilled_lint_expectations)]
use state_tree::{StateTreeArgs, StateTreeContext, StateTreeEvent, StateTreeInstance};

fn integration<E: StateTreeEvent>(
    tree: &StateTreeInstance,
    context: StateTreeContext<'_>,
    args: &mut StateTreeArgs,
) {
    let _ = tree.status();
    let _ = tree.definition();
    let _ = tree.definition_id();
    let _ = StateTreeInstance::new(Default::default());
    let _ = StateTreeInstance::new_with_initial_data(Default::default(), Default::default());
    let _ = context.entity();
    let _ = context.event_name();
    let _ = context.external_event();
    let _ = context.event::<E>();
    let _ = args.take::<bool>("placing");
    let _ = args.take_or::<bool>("placing", false);
    let _ = args.iter();
}

#[cfg_attr(
    dylint_lib = "inslag_state_tree_private_access",
    allow(inslag_state_tree_private_access, reason = "generic binding adapter")
)]
fn adapter(tree: &StateTreeInstance) {
    let _ = tree.data("placing");
    let _ = tree.active_state_names();
}

#[cfg_attr(
    dylint_lib = "inslag_state_tree_private_access",
    expect(inslag_state_tree_private_access, reason = "intentional inspection")
)]
fn expected(context: StateTreeContext<'_>) {
    let _ = context.data("placing");
}

fn expression_allow(tree: &StateTreeInstance) {
    #[cfg_attr(
        dylint_lib = "inslag_state_tree_private_access",
        allow(inslag_state_tree_private_access)
    )]
    let _ = tree.data("placing");
}

mod unrelated {
    pub struct StateTreeInstance;
    impl StateTreeInstance {
        pub fn data(&self, _: &str) {}
        pub fn active_states(&self) {}
        pub fn active_state_names(&self) {}
    }
    pub struct StateTreeContext;
    impl StateTreeContext {
        pub fn data(&self, _: &str) {}
    }
}

fn unrelated() {
    let tree = unrelated::StateTreeInstance;
    tree.data("placing");
    tree.active_states();
    tree.active_state_names();
    unrelated::StateTreeInstance::data(&tree, "placing");
    let _ = unrelated::StateTreeInstance::data;
    unrelated::StateTreeContext.data("placing");
}

fn main() {}
