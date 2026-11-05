use state_tree::StateTreeInstance;

fn inspect(tree: &StateTreeInstance) {
    let _ = tree.data("placing");
    let _ = tree.active_states();
}
