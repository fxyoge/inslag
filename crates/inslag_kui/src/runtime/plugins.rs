use super::*;

struct KuiAssetPlugin;

impl Plugin for KuiAssetPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<KuiAsset>()
            .init_asset::<StyleSheet>()
            .register_asset_loader(KuiAssetLoader);
    }
}

pub(crate) struct KuiPlugin;

impl Plugin for KuiPlugin {
    fn finish(&self, app: &mut App) {
        crate::motion::install(app);
    }

    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<KuiAssetPlugin>() {
            app.add_plugins(KuiAssetPlugin);
        }
        // Register before Flair's loader snapshots its property registry in
        // finish(). finish() below covers headless/test apps adding Flair later.
        crate::motion::install(app);
        crate::interaction::install(app);
        app.configure_sets(
            Update,
            (
                KuiSystems::Prepare,
                KuiSystems::Mount,
                KuiSystems::Bind,
                KuiSystems::Outcomes,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (fail_assets, reload_assets, spawn_views)
                .chain()
                .in_set(KuiSystems::Mount),
        )
        .add_systems(
            Update,
            (collections::reconcile, sync_views)
                .chain()
                .in_set(KuiSystems::Bind),
        )
        .add_systems(PostUpdate, restore_display.before(StyleSystems::Prepare))
        .add_systems(
            PostUpdate,
            apply_visibility
                .after(restore_display)
                .after(StyleSystems::ApplyComputedProperties)
                .before(bevy::ui::UiSystems::Content),
        )
        .add_systems(
            PostUpdate,
            apply_meters
                .after(StyleSystems::ApplyComputedProperties)
                .before(bevy::ui::UiSystems::Content),
        );
    }
}
