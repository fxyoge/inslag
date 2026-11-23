use bevy_flair::style::StyleSystems;

use super::*;
use crate::KuiSystems;

pub(crate) struct KuiMotionPlugin;

impl Plugin for KuiMotionPlugin {
    fn build(&self, app: &mut App) {
        // Flair snapshots the property registry when it finishes.
        register_properties(app);
        app.add_systems(
            Update,
            restore_world
                .in_set(RestorePresentation)
                .before(KuiSystems::Prepare)
                .run_if(resource_exists::<MotionProperties>),
        )
        .add_systems(
            PostUpdate,
            (
                restore_layout.before(StyleSystems::Prepare),
                connect_timelines
                    .after(StyleSystems::ResolveAnimations)
                    .before(StyleSystems::SetAnimationValues),
                (apply_world, apply_layout_origin)
                    .after(StyleSystems::ApplyComputedProperties)
                    .before(bevy::ui::UiSystems::Content)
                    .before(bevy::transform::TransformSystems::Propagate),
            )
                .run_if(resource_exists::<MotionProperties>),
        );
    }

    fn finish(&self, app: &mut App) {
        register_properties(app);
    }
}
