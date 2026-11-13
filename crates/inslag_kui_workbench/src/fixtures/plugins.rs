use super::*;

pub(crate) struct FixturePlugin;

impl Plugin for FixturePlugin {
    fn build(&self, app: &mut App) {
        app.add_state_tree_event::<FixtureChanged>()
            .add_state_tree_effect::<ToolAction>("workbench.command");
    }
}
