use super::*;

/// Enables App-facing semantic trace collection.
///
/// Without this plugin, state-tree systems do not allocate, lock, or perform
/// runtime work for individual trace records.
#[derive(Default)]
pub struct StateTreeTracePlugin;

impl Plugin for StateTreeTracePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StateTreeTraceLog>();
    }
}
