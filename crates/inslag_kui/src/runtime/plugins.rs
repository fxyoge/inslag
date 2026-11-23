use super::*;
use crate::{interaction::KuiInteractionPlugin, motion::KuiMotionPlugin};

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
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<KuiAssetPlugin>() {
            app.add_plugins(KuiAssetPlugin);
        }
        app.add_plugins((KuiMotionPlugin, KuiInteractionPlugin))
            .configure_sets(
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
