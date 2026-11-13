use super::*;

/// Installs the application-wide state-tree runtime into a Bevy schedule.
///
/// The plugin processes each instance's events sequentially but processes
/// separate instances in parallel. Event-producing systems should run in
/// [`StateTreeSet::QueueEvents`] for same-schedule delivery. Events written
/// later remain queued for the next run.
pub struct StateTreePlugin {
    schedule: InternedScheduleLabel,
}

impl StateTreePlugin {
    /// Creates the runtime that processes events in `schedule`.
    pub fn new(schedule: impl ScheduleLabel) -> Self {
        Self {
            schedule: schedule.intern(),
        }
    }
}

impl Plugin for StateTreePlugin {
    fn build(&self, app: &mut App) {
        assert!(
            !app.world().contains_resource::<StateTreeEventIngress>(),
            "StateTreePlugin was added more than once"
        );
        assert!(
            app.world().contains_resource::<Time>(),
            "StateTreePlugin requires Bevy's TimePlugin to be installed first"
        );

        if !app.world().contains_resource::<StateTreeAssetsInstalled>() {
            assert!(
                app.world().contains_resource::<AssetServer>(),
                "StateTreePlugin requires Bevy's AssetPlugin to be installed first"
            );
            if !app
                .world()
                .contains_resource::<Assets<StateTreeDefinition>>()
            {
                app.init_asset::<StateTreeDefinition>();
            }
            app.register_asset_loader(ScxmlLoader)
                .insert_resource(StateTreeAssetsInstalled);
        }

        app.init_resource::<StateTreeEventIngress>()
            .insert_resource(StateTreeSchedule(self.schedule))
            .init_resource::<StateTreeAppEventRegistry>()
            .init_resource::<crate::StateTreeProtocolInventory>()
            .init_resource::<StateTreeTimers>()
            .init_resource::<LinkedStateTreeDefinitions>()
            .init_resource::<StateTreeEffectRegistry>()
            .init_resource::<StateTreeActivityRegistry>()
            .init_resource::<StateTreeActivityIngress>()
            .init_resource::<StateTreeSettings>()
            .configure_sets(
                self.schedule,
                (
                    StateTreeSet::Reconstruct,
                    StateTreeSet::ProcessTimers,
                    StateTreeSet::QueueEvents,
                    StateTreeSet::ProcessActivityOutcomes,
                    StateTreeSet::Initialize,
                    StateTreeSet::ProcessEvents,
                    StateTreeSet::HandleActivities,
                    StateTreeSet::HandleEffects,
                )
                    .chain(),
            )
            .add_systems(
                self.schedule,
                (
                    reconstruct_changed
                        .in_set(StateTreeSet::Reconstruct)
                        .run_if(
                            resource_changed::<Assets<StateTreeDefinition>>
                                .or_eager(on_message::<AssetEvent<StateTreeDefinition>>)
                                .or_eager(
                                    any_match_filter::<(
                                        Changed<StateTreeConfiguration>,
                                        Allow<Disabled>,
                                    )>,
                                )
                                .or_eager(any_component_removed::<StateTreeConfiguration>),
                        ),
                    process_timers.in_set(StateTreeSet::ProcessTimers),
                    process_activity_outcomes.in_set(StateTreeSet::ProcessActivityOutcomes),
                    initialize_pending.in_set(StateTreeSet::Initialize),
                    process_events.in_set(StateTreeSet::ProcessEvents),
                    finish_teardown.after(StateTreeSet::HandleEffects),
                ),
            );

        crate::spawn_activity::register(app, self.schedule);
        crate::binding::install_pending(app.world_mut());
        app.add_systems(
            bevy_app::First,
            crate::binding::install_pending.run_if(crate::binding::has_pending),
        )
        .add_systems(
            bevy_app::Last,
            crate::binding::install_pending.run_if(crate::binding::has_pending),
        );
        #[cfg(feature = "persistence")]
        crate::persistence::ensure_adapter(app);
    }
}
