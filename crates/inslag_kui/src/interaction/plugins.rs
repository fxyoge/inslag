use super::*;
use crate::KuiSystems;

pub(crate) struct KuiInteractionPlugin;

impl Plugin for KuiInteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Drags>()
            .init_resource::<Hovers>()
            .add_observer(click)
            .add_observer(press)
            .add_observer(release)
            .add_observer(drag_start)
            .add_observer(drag_move)
            .add_observer(drag_end)
            .add_observer(cancel)
            .add_systems(
                Update,
                (cancel_unavailable, hover, cursor)
                    .chain()
                    .in_set(KuiSystems::Input)
                    .before(KuiSystems::Prepare),
            )
            .add_systems(
                PreUpdate,
                text_hits.in_set(bevy::picking::PickingSystems::Backend),
            );
    }
}
