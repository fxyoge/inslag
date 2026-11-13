use super::*;

pub(super) struct WorkbenchPlugin {
    asset_root: PathBuf,
    pub(super) story_root: Option<PathBuf>,
    pub(super) memory: Dir,
}

impl WorkbenchPlugin {
    pub(super) fn new(asset_root: impl Into<PathBuf>) -> Self {
        Self {
            asset_root: asset_root.into(),
            story_root: None,
            memory: Dir::default(),
        }
    }
}

impl Plugin for WorkbenchPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((fixtures::FixturePlugin, EnhancedInputPlugin))
            .add_input_context::<editor::Editor>();
        let mut tool = Workbench::new(self.asset_root.clone());
        tool.story_root = self.story_root.clone();
        tool.memory = self.memory.clone();
        app.insert_resource(tool)
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (apply_actions, update)
                    .chain()
                    .in_set(StateTreeSet::HandleEffects),
            )
            .add_systems(
                Update,
                preview::attach.after(StateTreeSet::HandleActivities),
            )
            .add_systems(
                PostUpdate,
                preview::layout.after(bevy::ui::UiSystems::Layout),
            )
            .add_systems(
                First,
                preview::pointer.in_set(bevy::picking::PickingSystems::PostInput),
            )
            .add_systems(
                PostUpdate,
                editor::finish.after(bevy::text::EditableTextSystems),
            )
            .add_observer(editor::accept)
            .add_observer(editor::commit)
            .add_observer(editor::close)
            .add_observer(editor::changed)
            .add_observer(scroll)
            .add_observer(resize_panes);
    }
}
