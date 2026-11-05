#![allow(dead_code)]
use state_tree::StateTreeInstance;

#[cfg(test)]
mod white_box {
    use super::*;
    fn helper(tree: &StateTreeInstance) {
        let _ = tree.data("placing");
        let _ = tree.active_states();
        let _ = tree.active_state_names();
    }

    mod nested {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/support/helper.rs"
        ));
    }
}

#[test]
fn white_box_function() {
    fn helper(tree: &StateTreeInstance) {
        let _ = tree.data("placing");
    }
}

#[cfg(test)]
mod external_white_box {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/helper.rs"
    ));
}

// Production source must still warn under --test, even inside closures/helpers.
fn production(tree: &StateTreeInstance) {
    let _ = tree.data("placing");
    let _ = || tree.active_states();
}

fn main() {}
