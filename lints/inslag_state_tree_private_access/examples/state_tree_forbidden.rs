#![allow(dead_code)]
use state_tree::{
    StateTreeActivityContext, StateTreeContext, StateTreeEffectContext, StateTreeInstance,
};

fn instance(tree: &StateTreeInstance, key: &str) {
    let _ = tree.data("placing");
    let _ = tree.data(key);
    let _ = tree.active_states();
    let _ = tree.active_state_names();
}

fn context(context: StateTreeContext<'_>) {
    let _ = context.data("placing");
}

fn effect(context: StateTreeEffectContext<'_>) {
    let _ = context.data("placing");
}

fn activity(context: StateTreeActivityContext<'_>) {
    let _ = context.data("placing");
}

fn query(instances: ecs::prelude::Query<&StateTreeInstance>) {
    for tree in &instances {
        let _ = tree.data("placing");
    }
}

fn change_detection(tree: ecs::change_detection::Ref<StateTreeInstance>) {
    let _ = tree.active_state_names();
}

fn main() {}
