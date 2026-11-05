#![allow(dead_code)]
use state_tree::StateTreeInstance as Instance;

type Tree = Instance;
mod facade {
    pub use state_tree::StateTreeInstance as Controller;
}

fn qualified(tree: &Tree, context: state_tree::StateTreeContext<'_>) {
    let _ = state_tree::StateTreeInstance::data(tree, "placing");
    let _ = Instance::active_states(tree);
    let _ = Tree::active_state_names(tree);
    let _ = facade::Controller::data(tree, "placing");
    let _ = state_tree::StateTreeEffectContext::data(&context, "placing");
}

fn function_values(tree: &Instance) {
    let read = Instance::data;
    let _ = read(tree, "placing");
    let _ = Instance::active_states;
    let _ = Instance::active_state_names;
    let _ = state_tree::StateTreeContext::data;
    let _ = state_tree::StateTreeActivityContext::data;
}

fn reexported_receiver(tree: &facade::Controller) {
    let _ = tree.data("placing");
}

fn main() {}
