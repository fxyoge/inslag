use super::*;

/// Register KUI activities, effects, and events in the `Update` schedule.
///
/// Add after [`inslag_state_tree::StateTreePlugin`] and [`bevy_flair::FlairPlugin`].
#[derive(Default)]
pub struct KuiStateTreePlugin;

impl Plugin for KuiStateTreePlugin {
    fn build(&self, app: &mut App) {
        assert!(
            app.is_plugin_added::<inslag_state_tree::StateTreePlugin>(),
            "install StateTreePlugin before KuiStateTreePlugin"
        );
        app.add_state_tree_event::<UiAction>()
            .add_state_tree_event::<UiReady>()
            .add_state_tree_event::<UiFocusChanged>()
            .add_state_tree_activity(
                inslag_state_tree::Activity::<UiRender>::new("ui.render").with_lifecycle(),
            )
            .add_state_tree_effect::<FocusView>("ui.focus")
            .add_state_tree_effect::<ActivateView>("ui.activate")
            .init_resource::<ViewIndex>();
        if !app.is_plugin_added::<bevy::input_focus::InputFocusPlugin>() {
            app.add_plugins(bevy::input_focus::InputFocusPlugin);
        }
        if !app.is_plugin_added::<KuiPlugin>() {
            app.add_plugins(KuiPlugin);
        }
        app.configure_sets(
            Update,
            (
                KuiSystems::Prepare,
                KuiSystems::Mount,
                KuiSystems::Bind,
                KuiSystems::Outcomes,
            )
                .in_set(StateTreeSet::HandleActivities),
        )
        .configure_sets(
            Update,
            crate::KuiSystems::Input.before(StateTreeSet::QueueEvents),
        )
        .add_systems(
            Update,
            (manage_activities, supply_data)
                .chain()
                .in_set(KuiSystems::Prepare),
        )
        .add_systems(Update, report_failures.in_set(KuiSystems::Outcomes))
        .add_systems(
            Update,
            (focus_view, activate_view)
                .chain()
                .in_set(StateTreeSet::HandleEffects),
        )
        .add_systems(PostUpdate, report_ready.after(bevy::ui::UiSystems::Layout))
        .add_observer(forward_action)
        .add_observer(remove_view);
    }
}
