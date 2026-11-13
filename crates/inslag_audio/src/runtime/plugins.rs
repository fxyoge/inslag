use super::*;
use bevy_ecs::schedule::{InternedScheduleLabel, ScheduleLabel};

/// Register SCXML audio effects, activities, and mixer resources.
pub struct EnhancedAudioPlugin {
    schedule: InternedScheduleLabel,
}

impl EnhancedAudioPlugin {
    /// Use the schedule passed to `StateTreePlugin::new`.
    pub fn new(schedule: impl ScheduleLabel) -> Self {
        Self {
            schedule: schedule.intern(),
        }
    }
}

impl Plugin for EnhancedAudioPlugin {
    fn build(&self, app: &mut App) {
        assert!(
            app.is_plugin_added::<StateTreePlugin>(),
            "install StateTreePlugin before EnhancedAudioPlugin"
        );
        assert!(
            app.is_plugin_added::<AudioPlugin>(),
            "install Bevy's AudioPlugin before EnhancedAudioPlugin"
        );
        assert!(app.world().contains_resource::<AssetServer>());
        assert!(
            app.world().contains_resource::<Assets<AudioSource>>(),
            "enable at least one audio format"
        );
        assert!(app.world().contains_resource::<Time>());
        let schedule = self.schedule;
        app.add_state_tree_effect::<Play>("audio.play")
            .add_state_tree_activity(
                inslag_state_tree::Activity::<Playback>::new("audio.playback").with_lifecycle(),
            )
            .add_state_tree_effect::<BusGain>("audio.set_bus_gain");
        app.init_resource::<AudioSettings>()
            .init_resource::<AudioMixer>()
            .init_resource::<Runtime>()
            .add_message::<AudioDiagnostic>()
            .configure_sets(
                schedule,
                (
                    EnhancedAudioSet::Collect.in_set(StateTreeSet::HandleEffects),
                    EnhancedAudioSet::Playback.after(StateTreeSet::HandleEffects),
                )
                    .chain(),
            )
            .add_systems(
                schedule,
                collect_activities.in_set(StateTreeSet::HandleActivities),
            )
            .add_systems(schedule, collect_effects.in_set(EnhancedAudioSet::Collect))
            .add_systems(schedule, advance.in_set(EnhancedAudioSet::Playback));
    }
}
