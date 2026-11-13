use std::{
    collections::HashMap,
    hash::Hash,
    marker::PhantomData,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use bevy_ecs::{
    entity::Entity,
    message::Message,
    prelude::{Component, Query, Res, Resource},
    system::{ParallelCommands, SystemParam},
    world::{DeferredWorld, World},
};

mod registration;
mod runs;

pub use registration::{Activity, ActivityCollectionRetention};
pub use runs::{ActivityRun, ActivityRuns};

use crate::{
    LuauValue, StateTreeActivityArgs, StateTreeEffectContext, StateTreeEffectError, StateTreeInput,
    StateTreeInstance, definition::ActivityId, protocol::RegistrationOrigin,
};

/// A typed long-running request owned by one active state.
pub trait StateTreeActivity: Send + Sync + 'static {
    type Output: Send + Sync + 'static;
    type Error: Send + Sync + 'static;

    fn output_data(_output: &Self::Output) -> Option<LuauValue> {
        None
    }

    fn error_data(_error: &Self::Error) -> Option<LuauValue> {
        None
    }
}

/// A generation-safe handle to one activity run on one entity.
pub struct StateTreeActivityToken<A> {
    pub(crate) entity: Entity,
    pub(crate) runtime_id: u64,
    pub(crate) activity: ActivityId,
    pub(crate) generation: u64,
    marker: PhantomData<fn() -> A>,
}

impl<A> Copy for StateTreeActivityToken<A> {}
impl<A> Clone for StateTreeActivityToken<A> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<A> std::fmt::Debug for StateTreeActivityToken<A> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StateTreeActivityToken")
            .field("entity", &self.entity)
            .field("runtime_id", &self.runtime_id)
            .field("activity", &self.activity)
            .field("generation", &self.generation)
            .finish()
    }
}

impl<A> PartialEq for StateTreeActivityToken<A> {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
            && self.runtime_id == other.runtime_id
            && self.activity == other.activity
            && self.generation == other.generation
    }
}
impl<A> Eq for StateTreeActivityToken<A> {}
impl<A> std::hash::Hash for StateTreeActivityToken<A> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.entity.hash(state);
        self.runtime_id.hash(state);
        self.activity.hash(state);
        self.generation.hash(state);
    }
}
impl<A> PartialOrd for StateTreeActivityToken<A> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<A> Ord for StateTreeActivityToken<A> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.entity, self.runtime_id, self.activity, self.generation).cmp(&(
            other.entity,
            other.runtime_id,
            other.activity,
            other.generation,
        ))
    }
}

impl<A> StateTreeActivityToken<A> {
    pub fn entity(self) -> Entity {
        self.entity
    }

    pub(crate) fn erased(self) -> ErasedActivityToken {
        ErasedActivityToken {
            entity: self.entity,
            runtime_id: self.runtime_id,
            activity: self.activity,
            generation: self.generation,
        }
    }
}

/// Typed lifecycle messages for one registered activity request type.
pub enum StateTreeActivityLifecycle<A: StateTreeActivity> {
    Started {
        token: StateTreeActivityToken<A>,
        activity: A,
    },
    Cancelled {
        token: StateTreeActivityToken<A>,
    },
}

impl<A: StateTreeActivity> Message for StateTreeActivityLifecycle<A> {}

pub(crate) trait RegisteredActivity: Send + Sync {
    fn payload_type_id(&self) -> std::any::TypeId;
    #[cfg(feature = "persistence")]
    fn discard_pending_starts(&self, world: &mut bevy_ecs::world::World);
    fn start(
        &self,
        context: StateTreeEffectContext<'_>,
        arguments: StateTreeActivityArgs,
        token: ErasedActivityToken,
        commands: &ParallelCommands<'_, '_>,
    ) -> Result<(), StateTreeEffectError>;
    fn cancel(&self, token: ErasedActivityToken, commands: &ParallelCommands<'_, '_>);
    fn finish(&self, token: ErasedActivityToken, commands: &ParallelCommands<'_, '_>);
    fn cancel_deferred(&self, token: ErasedActivityToken, world: &mut DeferredWorld<'_>);
}

pub(crate) trait RuntimeActivitySink {
    fn start(&mut self, request: RuntimeActivityStart<'_>) -> Result<(), StateTreeEffectError>;
    fn cancel(&mut self, handler: &Arc<dyn RegisteredActivity>, token: ErasedActivityToken);
    fn finish(&mut self, handler: &Arc<dyn RegisteredActivity>, token: ErasedActivityToken);
}

pub(crate) struct RuntimeActivityStart<'a> {
    pub handler: &'a Arc<dyn RegisteredActivity>,
    pub context: StateTreeEffectContext<'a>,
    pub arguments: StateTreeActivityArgs,
    pub token: ErasedActivityToken,
}

pub(crate) struct BevyActivitySink<'a, 'w, 's> {
    pub commands: &'a ParallelCommands<'w, 's>,
}

impl RuntimeActivitySink for BevyActivitySink<'_, '_, '_> {
    fn start(&mut self, request: RuntimeActivityStart<'_>) -> Result<(), StateTreeEffectError> {
        request.handler.start(
            request.context,
            request.arguments,
            request.token,
            self.commands,
        )
    }
    fn cancel(&mut self, handler: &Arc<dyn RegisteredActivity>, token: ErasedActivityToken) {
        handler.cancel(token, self.commands);
    }

    fn finish(&mut self, handler: &Arc<dyn RegisteredActivity>, token: ErasedActivityToken) {
        handler.finish(token, self.commands);
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ErasedActivityToken {
    pub entity: Entity,
    pub runtime_id: u64,
    pub activity: ActivityId,
    pub generation: u64,
}

impl ErasedActivityToken {
    pub(crate) fn typed<A>(self) -> StateTreeActivityToken<A> {
        StateTreeActivityToken {
            entity: self.entity,
            runtime_id: self.runtime_id,
            activity: self.activity,
            generation: self.generation,
            marker: PhantomData,
        }
    }
}

struct TypedActivity<A> {
    markers: Arc<[registration::MarkerBinding]>,
    collection: bool,
    retention: ActivityCollectionRetention,
    clone_activity: Option<fn(&A) -> A>,
    lifecycle: bool,
}

impl<A: StateTreeActivity> TypedActivity<A> {
    fn enqueue_stop(
        &self,
        token: ErasedActivityToken,
        commands: &ParallelCommands<'_, '_>,
        lifecycle: bool,
    ) {
        if self.markers.is_empty() && !self.collection && !lifecycle {
            return;
        }
        let markers = Arc::clone(&self.markers);
        let collection = self.collection;
        let retention = self.retention;
        commands.command_scope(move |mut commands| {
            commands.queue(move |world: &mut World| {
                stop::<A>(world, token, &markers, collection, retention, lifecycle);
            });
        });
    }
}

impl<A> RegisteredActivity for TypedActivity<A>
where
    A: StateTreeActivity + StateTreeInput,
{
    fn payload_type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<A>()
    }
    #[cfg(feature = "persistence")]
    fn discard_pending_starts(&self, world: &mut bevy_ecs::world::World) {
        if !self.lifecycle {
            return;
        }
        world.resource_scope(
            |world,
             mut messages: bevy_ecs::prelude::Mut<
                bevy_ecs::message::Messages<StateTreeActivityLifecycle<A>>,
            >| {
                let mut cursor = messages.get_cursor();
                // Reissuing retained messages would replay starts to readers that
                // have already consumed them. Keep their original message identities.
                for message in cursor.read_mut(&mut messages) {
                    if let StateTreeActivityLifecycle::Started { token, .. } = message
                        && (world
                            .get::<crate::PersistStateTree>(token.entity())
                            .is_some()
                            || world.get::<StateTreeInstance>(token.entity()).is_none_or(
                                |instance| !instance.accepts_activity_token(token.erased()),
                            ))
                    {
                        *message = StateTreeActivityLifecycle::Cancelled { token: *token };
                    }
                }
            },
        );
    }
    fn start(
        &self,
        context: StateTreeEffectContext<'_>,
        mut arguments: StateTreeActivityArgs,
        token: ErasedActivityToken,
        commands: &ParallelCommands<'_, '_>,
    ) -> Result<(), StateTreeEffectError> {
        let activity = A::from_state_tree_input(&context, &mut arguments)?;
        arguments.finish()?;
        if self.markers.is_empty() && !self.collection && !self.lifecycle {
            return Ok(());
        }
        let markers = Arc::clone(&self.markers);
        let collection = self.collection;
        let clone_activity = self.clone_activity;
        let lifecycle = self.lifecycle;
        commands.command_scope(move |mut commands| {
            commands.queue(move |world: &mut World| {
                let active = world
                    .get::<StateTreeInstance>(token.entity)
                    .is_some_and(|instance| instance.accepts_activity_token(token));
                if active {
                    for marker in markers.iter() {
                        (marker.start)(world, token);
                    }
                }
                if lifecycle {
                    if collection && active {
                        world.write_message(StateTreeActivityLifecycle::<A>::Started {
                            token: token.typed(),
                            activity: clone_activity.expect("collection installs cloning")(
                                &activity,
                            ),
                        });
                    } else {
                        world.write_message(StateTreeActivityLifecycle::<A>::Started {
                            token: token.typed(),
                            activity,
                        });
                        return;
                    }
                }
                if collection && active {
                    runs::insert(world, token, activity);
                }
            });
        });
        Ok(())
    }

    fn cancel(&self, token: ErasedActivityToken, commands: &ParallelCommands<'_, '_>) {
        self.enqueue_stop(token, commands, self.lifecycle);
    }

    fn finish(&self, token: ErasedActivityToken, commands: &ParallelCommands<'_, '_>) {
        self.enqueue_stop(token, commands, false);
    }

    fn cancel_deferred(&self, token: ErasedActivityToken, world: &mut DeferredWorld<'_>) {
        if self.markers.is_empty() && !self.collection && !self.lifecycle {
            return;
        }
        let markers = Arc::clone(&self.markers);
        let collection = self.collection;
        let retention = self.retention;
        let lifecycle = self.lifecycle;
        world.commands().queue(move |world: &mut World| {
            stop::<A>(world, token, &markers, collection, retention, lifecycle);
        });
    }
}

fn stop<A: StateTreeActivity>(
    world: &mut World,
    token: ErasedActivityToken,
    markers: &[registration::MarkerBinding],
    collection: bool,
    retention: ActivityCollectionRetention,
    lifecycle: bool,
) {
    for marker in markers {
        (marker.stop)(world, token);
    }
    if collection {
        runs::remove::<A>(world, token, retention);
    }
    if lifecycle {
        world.write_message(StateTreeActivityLifecycle::<A>::Cancelled {
            token: token.typed(),
        });
    }
}

#[derive(Resource, Default)]
struct CollectionSettings(HashMap<std::any::TypeId, ActivityCollectionRetention>);

#[derive(Resource, Default)]
pub(crate) struct StateTreeActivityRegistry {
    activities: HashMap<Box<str>, Arc<dyn RegisteredActivity>>,
}

impl StateTreeActivityRegistry {
    #[cfg(feature = "persistence")]
    pub(crate) fn discard_pending_starts(&self, world: &mut bevy_ecs::world::World) {
        for handler in self.activities.values() {
            handler.discard_pending_starts(world);
        }
    }
    pub(crate) fn get(&self, name: &str) -> Option<Arc<dyn RegisteredActivity>> {
        self.activities.get(name).cloned()
    }
}

pub(crate) enum ActivityOutcomeKind {
    Complete,
    Fail,
}

pub(crate) struct ActivityOutcome {
    pub token: ErasedActivityToken,
    pub kind: ActivityOutcomeKind,
    pub data: Option<LuauValue>,
}

#[derive(Component)]
pub(crate) struct StateTreeActivityInbox {
    pub outcomes: Mutex<Vec<ActivityOutcome>>,
}

impl Default for StateTreeActivityInbox {
    fn default() -> Self {
        Self {
            outcomes: Mutex::new(Vec::new()),
        }
    }
}

#[derive(Resource)]
pub(crate) struct StateTreeActivityIngress {
    pub dirty: Mutex<bevy_ecs::entity::EntityHashSet>,
    pub dirty_hint: AtomicBool,
}

impl Default for StateTreeActivityIngress {
    fn default() -> Self {
        Self {
            dirty: Mutex::new(bevy_ecs::entity::EntityHashSet::default()),
            dirty_hint: AtomicBool::new(false),
        }
    }
}

/// Writes terminal outcomes for one typed activity request.
#[derive(SystemParam)]
pub struct StateTreeActivityOutcomeWriter<'w, 's, A: StateTreeActivity> {
    ingress: Res<'w, StateTreeActivityIngress>,
    instances: Query<'w, 's, (&'static StateTreeInstance, &'static StateTreeActivityInbox)>,
    marker: PhantomData<A>,
}

impl<A: StateTreeActivity> StateTreeActivityOutcomeWriter<'_, '_, A> {
    /// Projects and enqueues a completion when `token` still names an active run.
    ///
    /// Returns `true` only when the outcome was accepted and enqueued.
    pub fn complete(&mut self, token: StateTreeActivityToken<A>, output: A::Output) -> bool {
        self.write(token, ActivityOutcomeKind::Complete, || {
            A::output_data(&output)
        })
    }

    /// Projects and enqueues a failure when `token` still names an active run.
    ///
    /// Returns `true` only when the outcome was accepted and enqueued.
    pub fn fail(&mut self, token: StateTreeActivityToken<A>, error: A::Error) -> bool {
        self.write(token, ActivityOutcomeKind::Fail, || A::error_data(&error))
    }

    fn write<F>(
        &mut self,
        token: StateTreeActivityToken<A>,
        kind: ActivityOutcomeKind,
        project: F,
    ) -> bool
    where
        F: FnOnce() -> Option<LuauValue>,
    {
        let Ok((instance, inbox)) = self.instances.get(token.entity) else {
            return false;
        };
        let erased = ErasedActivityToken {
            entity: token.entity,
            runtime_id: token.runtime_id,
            activity: token.activity,
            generation: token.generation,
        };
        if !instance.accepts_activity_token(erased) {
            return false;
        }
        let mut outcomes = inbox
            .outcomes
            .lock()
            .expect("state-tree activity inbox poisoned");
        if outcomes.iter().any(|outcome| outcome.token == erased) {
            return false;
        }
        outcomes.push(ActivityOutcome {
            token: erased,
            kind,
            data: project(),
        });
        drop(outcomes);
        self.ingress
            .dirty
            .lock()
            .expect("state-tree activity dirty set poisoned")
            .insert(token.entity);
        self.ingress.dirty_hint.store(true, Ordering::Release);
        true
    }
}

pub(crate) fn register_activity<A>(
    world: &mut bevy_ecs::world::World,
    activity: Activity<A>,
    origin: RegistrationOrigin,
) where
    A: StateTreeActivity + StateTreeInput,
{
    let Activity {
        name,
        markers,
        collection,
        retention,
        clone_activity,
        lifecycle,
    } = activity;
    assert!(
        !name.is_empty(),
        "state-tree activity names cannot be empty"
    );
    let registry = world.get_resource_or_init::<StateTreeActivityRegistry>();
    if registry.activities.contains_key(&name) {
        panic!("state-tree activity `{name}` was registered more than once");
    }
    if collection {
        let mut settings = world.get_resource_or_init::<CollectionSettings>();
        let previous = settings
            .0
            .entry(std::any::TypeId::of::<A>())
            .or_insert(retention);
        assert_eq!(
            *previous, retention,
            "activity collections of the same type must use the same retention policy"
        );
    }
    if lifecycle {
        crate::app::add_message::<StateTreeActivityLifecycle<A>>(world);
    }
    world
        .resource_mut::<StateTreeActivityRegistry>()
        .activities
        .insert(
            name.clone(),
            Arc::new(TypedActivity::<A> {
                markers: markers.into(),
                collection,
                retention,
                clone_activity,
                lifecycle,
            }),
        );
    world
        .get_resource_or_init::<crate::StateTreeProtocolInventory>()
        .add_activity(name, origin);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(crate::StateTreeInput)]
    struct TestActivity;

    impl StateTreeActivity for TestActivity {
        type Output = ();
        type Error = ();
    }

    #[test]
    fn spawn_name_can_be_registered_when_no_activity_owns_it() {
        let mut world = bevy_ecs::world::World::new();
        register_activity::<TestActivity>(
            &mut world,
            Activity::new("ist.spawn"),
            RegistrationOrigin::Application,
        );
        assert!(
            world
                .resource::<StateTreeActivityRegistry>()
                .get("ist.spawn")
                .is_some()
        );
    }

    #[test]
    #[should_panic(expected = "state-tree activity `duplicate` was registered more than once")]
    fn activity_names_must_be_unique() {
        let mut world = bevy_ecs::world::World::new();
        register_activity::<TestActivity>(
            &mut world,
            Activity::new("duplicate"),
            RegistrationOrigin::Application,
        );
        register_activity::<TestActivity>(
            &mut world,
            Activity::new("duplicate"),
            RegistrationOrigin::Application,
        );
    }

    #[test]
    fn repeated_old_cleanup_cannot_remove_new_bindings() {
        #[derive(bevy_ecs::prelude::Component, Default)]
        struct Marker;

        let mut world = World::new();
        let entity = world.spawn_empty().id();
        let old = ErasedActivityToken {
            entity,
            runtime_id: 1,
            activity: ActivityId(0),
            generation: 1,
        };
        let new = ErasedActivityToken {
            generation: 2,
            ..old
        };
        let activity = Activity::<TestActivity>::new("test").with_marker::<Marker>();
        let marker = activity.markers[0];
        (marker.start)(&mut world, old);
        runs::insert(&mut world, old, TestActivity);
        stop::<TestActivity>(
            &mut world,
            old,
            &activity.markers,
            true,
            ActivityCollectionRetention::RemoveWhenEmpty,
            false,
        );
        (marker.start)(&mut world, new);
        runs::insert(&mut world, new, TestActivity);
        stop::<TestActivity>(
            &mut world,
            old,
            &activity.markers,
            true,
            ActivityCollectionRetention::RemoveWhenEmpty,
            false,
        );
        assert!(world.get::<Marker>(entity).is_some());
        assert_eq!(
            world
                .get::<ActivityRuns<TestActivity>>(entity)
                .unwrap()
                .len(),
            1
        );
        assert!(
            world
                .get::<ActivityRuns<TestActivity>>(entity)
                .unwrap()
                .get(new.typed())
                .is_some()
        );
    }
}
