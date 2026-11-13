#![expect(dead_code)]

use bevy_app::{App, Plugin as Installation, PluginGroup, PluginGroupBuilder};

mod wiring {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/plugins.rs"
    ));
}
mod declarations {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/declarations.rs"
    ));
}

struct Misplaced;

impl Installation for Misplaced {
    fn build(&self, _: &mut App) {}
}

enum MisplacedGroup {
    Default,
}

impl PluginGroup for MisplacedGroup {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
    }
}

impl Installation for wiring::MisplacedImplementation {
    fn build(&self, _: &mut App) {}
}

type Alias = declarations::Aliased;

impl Installation for Alias {
    fn build(&self, _: &mut App) {}
}

struct Generic<T>(std::marker::PhantomData<T>);

impl<T: Send + Sync + 'static> Installation for Generic<T> {
    fn build(&self, _: &mut App) {}
}

bevy_app::plugin_group! {
    struct GeneratedGroup {}
}

macro_rules! plugin {
    ($name:ident) => {
        struct $name;
        impl Installation for $name {
            fn build(&self, _: &mut App) {}
        }
    };
}

plugin!(Generated);

mod plugins {
    use super::*;

    struct Inline;

    impl Installation for Inline {
        fn build(&self, _: &mut App) {}
    }
}

mod unrelated {
    trait Plugin {}
    trait PluginGroup {}
    struct NamedPlugin;
    impl Plugin for NamedPlugin {}
    impl PluginGroup for NamedPlugin {}
}

#[allow(bevy_standard_plugin_filename)]
mod exception {
    use super::*;
    plugin!(Allowed);
}

fn main() {}
