use super::*;

/// Loads `.loot` assets and processes loot requests.
///
/// Requires [`bevy_asset::AssetPlugin`] to be installed first.
#[derive(Default)]
pub struct LootPlugin {
    sources: Vec<AssetPath<'static>>,
}

impl LootPlugin {
    /// Adds `.loot` asset paths to load. Tables can reference tables in other configured files.
    ///
    /// Requests fail if any configured file cannot be loaded or compiled, or if no files are configured.
    pub fn with_sources(
        mut self,
        sources: impl IntoIterator<Item = impl Into<AssetPath<'static>>>,
    ) -> Self {
        self.sources.extend(sources.into_iter().map(Into::into));
        self
    }
}

impl Plugin for LootPlugin {
    fn build(&self, app: &mut App) {
        assert!(
            !app.world().contains_resource::<Runtime>(),
            "LootPlugin was installed twice"
        );
        assert!(
            app.world().contains_resource::<bevy_asset::AssetServer>(),
            "LootPlugin requires AssetPlugin first"
        );
        app.init_resource::<LootRegistry>()
            .init_resource::<ContextRegistrations>()
            .init_asset::<LootSource>()
            .init_asset_loader::<LootLoader>()
            .init_resource::<Runtime>()
            .add_message::<NativeRequest>()
            .add_message::<NativeFinished>()
            .add_message::<NativeCall>()
            .add_message::<NativeResult>()
            .add_message::<ContextCall>()
            .configure_sets(
                PostUpdate,
                (LootSet::Evaluate, LootSet::Generate, LootSet::Publish).chain(),
            )
            .add_systems(PreUpdate, prepare_sources)
            .add_systems(PostUpdate, drive.in_set(LootSet::Evaluate));
        let server = app.world().resource::<AssetServer>();
        let handles = self
            .sources
            .iter()
            .map(|path| server.load(path.clone()))
            .collect();
        app.insert_resource(Sources {
            handles,
            prepared: PreparedLoot::Loading,
            dirty: true,
            compiled: false,
        });
    }
}
