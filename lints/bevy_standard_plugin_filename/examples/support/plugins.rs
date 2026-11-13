use bevy_app::{App, Plugin, PluginGroup, PluginGroupBuilder};

pub struct Correct;

impl Plugin for Correct {
    fn build(&self, _: &mut App) {}
}

pub struct CorrectGroup;

impl PluginGroup for CorrectGroup {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
    }
}

bevy_app::plugin_group! {
    pub struct GeneratedGroup {}
}

pub struct MisplacedImplementation;

impl Plugin for super::declarations::MisplacedDeclaration {
    fn build(&self, _: &mut App) {}
}

macro_rules! plugin {
    () => {
        struct Generated;
        impl Plugin for Generated {
            fn build(&self, _: &mut App) {}
        }
    };
}

plugin!();
