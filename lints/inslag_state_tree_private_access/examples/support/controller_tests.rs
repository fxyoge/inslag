use state_tree::StateTreeContext;

fn inspect(context: StateTreeContext<'_>) {
    let _ = context.data("placing");
}
