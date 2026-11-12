//! Author behavior in SCXML, then attach it to Bevy entities.
//!
//! Load a definition once and reuse its handle. Each [`StateTreeInstance`] keeps
//! its own active states and data.
//!
//! # Load a state tree
//!
//! Save this as `assets/state_trees/controller.scxml`:
//!
//! ```xml
//! <scxml xmlns="http://www.w3.org/2005/07/scxml" initial="idle">
//!   <state id="idle">
//!     <transition event="wake" target="active"/>
//!   </state>
//!   <state id="active"/>
//! </scxml>
//! ```
//!
//! Register the event name and spawn an instance:
//!
//! ```no_run
//! use bevy_app::{App, Startup, TaskPoolPlugin, Update};
//! use bevy_asset::{AssetPlugin, AssetServer};
//! use bevy_ecs::prelude::*;
//! use bevy_time::TimePlugin;
//! use inslag_state_tree::prelude::*;
//!
//! #[derive(EntityEvent, StateTreeEvent)]
//! #[state_tree(name = "wake")]
//! struct Wake {
//!     entity: Entity,
//! }
//!
//! fn spawn(mut commands: Commands, assets: Res<AssetServer>) {
//!     let definition = assets.load("state_trees/controller.scxml");
//!     commands.spawn(StateTreeInstance::new(definition));
//! }
//!
//! let mut app = App::new();
//! app.add_plugins((
//!     TaskPoolPlugin::default(),
//!     TimePlugin,
//!     AssetPlugin::default(),
//!     StateTreePlugin::new(Update),
//! ))
//! .add_state_tree_event::<Wake>()
//! .add_systems(Startup, spawn);
//! ```
//!
//! Install [`StateTreePlugin`] after Bevy's time and asset plugins. Instances
//! wait for their assets to load; inspect [`StateTreeLinkFailure`] if one cannot
//! start because an event, effect, or activity is unregistered.
//!
//! # Send events
//!
//! Use [`EntityStateTreeEventWriter`] from your gameplay systems:
//!
//! ```
//! # use bevy_ecs::prelude::*;
//! # use inslag_state_tree::prelude::*;
//! # #[derive(EntityEvent, StateTreeEvent)]
//! # #[state_tree(name = "wake")]
//! # struct Wake { entity: Entity }
//! fn wake(entity: Entity, events: &mut EntityStateTreeEventWriter) {
//!     events.write(Wake { entity });
//! }
//! ```
//!
//! Put event-producing systems in [`StateTreeSet::QueueEvents`] for delivery in
//! the same schedule run. Order producers explicitly when their relative order
//! matters. Events for instances still waiting on assets at processing time are
//! discarded, not replayed later.
//!
//! For broadcasts, derive `Event` instead of `EntityEvent`, add
//! `#[state_tree(name = "...", broadcast)]`, and use [`BroadcastStateTreeEventWriter`].
//!
//! # Add data and delays
//!
//! Set `datamodel="luau"` and `xmlns:ist="urn:inslag-state-tree"` on `<scxml>`:
//!
//! ```xml
//! <datamodel><data id="health" expr="100"/></datamodel>
//! <state id="waiting">
//!   <transition ist:after="250ms" cond="health > 0" target="ready">
//!     <assign location="health" expr="health - 1"/>
//!   </transition>
//! </state>
//! <state id="ready"/>
//! ```
//!
//! Use `<assign>` to change declared data. Add [`StateTreeConfiguration`] to the
//! entity for read-only inputs such as `config.speed`. Timers start on state
//! entry and are cancelled on exit.
//!
//! # Connect gameplay
//!
//! Register effects with [`add_state_tree_effect`](StateTreeExt::add_state_tree_effect),
//! then emit them with `<ist:effect name="..."/>`. Read their typed Bevy messages
//! in [`StateTreeSet::HandleEffects`].
//!
//! For work that lasts until a state exits, register a [`StateTreeActivity`] with
//! [`add_state_tree_activity`](StateTreeExt::add_state_tree_activity) and use
//! `<ist:activity id="..." name="..."/>`. Handle its start and cancellation in
//! [`StateTreeSet::HandleActivities`].
//!
//! # Save tree data
//!
//! Enable `persistence`, install `inslag_persist::SavePlugin`, and add
//! [`PersistStateTree`] to the entities you want to save. Only datamodel values
//! persist: loading starts execution again from the current asset's initial
//! state, with fresh timers and activities.

extern crate self as inslag_state_tree;

mod activity;
mod app;
mod binding;
mod definition;
mod effect;
mod event;
mod input;
mod instance;
mod luau;
mod plugin;
mod protocol;
mod spawn_activity;
mod trace;

#[cfg(feature = "persistence")]
pub mod persistence;

#[cfg(test)]
mod test_harness;

pub mod scxml;

pub use app::StateTreeExt;
pub use binding::{StateTreeBinding, StateTreeBindingSet};
pub use definition::{
    BuildError, HistoryId, StateId, StateTreeAction, StateTreeActivityDefinition,
    StateTreeArgument, StateTreeBuilder, StateTreeData, StateTreeDefinition,
    StateTreeEffectArgument, StateTreeLinkError, StateTreeLinkIssue, StateTreeTarget,
    StateTreeTransition,
};
pub use effect::{
    FromStateTreeValue, StateTreeActivityArgs, StateTreeActivityContext, StateTreeActivityError,
    StateTreeArgs, StateTreeContext, StateTreeEffectArgs, StateTreeEffectContext,
    StateTreeEffectError,
};
pub use event::{BroadcastStateTreeEvent, EntityStateTreeEvent, StateTreeEvent, StateTreeEventRef};
pub use input::StateTreeInput;
pub use inslag_state_tree_macros::{StateTreeEvent, StateTreeInput};
pub use instance::{
    DatamodelOperation, InstanceStatus, PersistStateTree, RuntimeFault, StateTreeConfiguration,
    StateTreeDataOverrideError, StateTreeDataOverrides, StateTreeInstance,
};
pub use luau::LuauValue;
pub use plugin::{
    BroadcastStateTreeEventWriter, EntityStateTreeEventWriter, StateTreeLinkFailure,
    StateTreePlugin, StateTreeSet, StateTreeSettings,
};
pub use protocol::{
    StateTreeDefinitionProtocol, StateTreeProtocolAnalysis, StateTreeProtocolInventory,
    StateTreeProtocolIssue, analyze_state_tree_protocol,
};
pub(crate) use trace::{
    NoStateTreeTrace, StateTreeHistoryResolution, StateTreeTrace, StateTreeTraceRecord,
    StateTreeTraceTrigger,
};
pub use trace::{StateTreeEventLog, StateTreeTraceLog, StateTreeTracePlugin};

/// Common state-tree imports.
pub mod prelude {
    pub use crate::{
        BroadcastStateTreeEvent, BroadcastStateTreeEventWriter, BuildError, DatamodelOperation,
        EntityStateTreeEvent, EntityStateTreeEventWriter, FromStateTreeValue, HistoryId,
        InstanceStatus, LuauValue, PersistStateTree, RuntimeFault, StateId, StateTreeAction,
        StateTreeActivity, StateTreeActivityArgs, StateTreeActivityContext,
        StateTreeActivityDefinition, StateTreeActivityError, StateTreeActivityLifecycle,
        StateTreeActivityOutcomeWriter, StateTreeActivityToken, StateTreeArgs, StateTreeArgument,
        StateTreeBinding, StateTreeBindingSet, StateTreeBuilder, StateTreeConfiguration,
        StateTreeContext, StateTreeData, StateTreeDataOverrideError, StateTreeDataOverrides,
        StateTreeDefinition, StateTreeEffectArgs, StateTreeEffectArgument, StateTreeEffectContext,
        StateTreeEffectError, StateTreeEvent, StateTreeEventLog, StateTreeEventRef, StateTreeExt,
        StateTreeInput, StateTreeInstance, StateTreeLinkError, StateTreeLinkFailure,
        StateTreeLinkIssue, StateTreePlugin, StateTreeProtocolInventory, StateTreeSet,
        StateTreeSettings, StateTreeTarget, StateTreeTraceLog, StateTreeTracePlugin,
        StateTreeTransition,
    };
}
pub use activity::{
    StateTreeActivity, StateTreeActivityLifecycle, StateTreeActivityOutcomeWriter,
    StateTreeActivityToken,
};
