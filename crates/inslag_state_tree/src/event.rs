use std::{
    any::{Any, TypeId, type_name},
    collections::HashMap,
};

use bevy_ecs::event::{EntityEvent, Event};
use bevy_ecs::world::World;
use thiserror::Error;

use crate::{LuauValue, protocol::RegistrationOrigin};

/// One external event payload accepted by state trees.
///
/// Implementations are normally generated with
/// [`StateTreeEvent`](macro@crate::StateTreeEvent).
pub trait StateTreeEvent: Event {
    /// The SCXML event name associated with this payload type.
    const AUTHORED_NAME: &'static str;

    /// Projects this event's typed payload for lazy Luau `_event.data` access.
    fn event_data(&self) -> Option<LuauValue> {
        None
    }
}

/// Marks a payload as an entity-targeted state-tree event.
pub trait EntityStateTreeEvent: StateTreeEvent + EntityEvent {}

/// Marks a payload as a broadcast state-tree event.
pub trait BroadcastStateTreeEvent: StateTreeEvent {}

/// Invalid state-tree event protocol registration.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub(crate) enum StateTreeEventRegistrationError {
    /// The same Rust payload type was registered more than once.
    #[error("state-tree event payload type `{type_name}` is already registered")]
    DuplicateType { type_name: &'static str },
    /// Authored event names must contain at least one character.
    #[error("state-tree event payload type `{type_name}` has an empty authored name")]
    EmptyName { type_name: &'static str },
    /// One authored event name cannot identify multiple Rust payload types.
    #[error(
        "state-tree event name `{name}` is already registered for payload type `{registered_type_name}`"
    )]
    DuplicateName {
        name: &'static str,
        registered_type_name: &'static str,
    },
}

/// Builds the application's external event protocol.
pub(crate) struct StateTreeEventRegistry {
    entries: Vec<RegisteredEvent>,
    type_ids: HashMap<TypeId, RegisteredEventId>,
    names: HashMap<&'static str, &'static str>,
    authored_names: Vec<&'static str>,
}

#[derive(bevy_ecs::prelude::Resource, Default)]
pub(crate) struct StateTreeAppEventRegistry(StateTreeEventRegistry);

impl StateTreeAppEventRegistry {
    pub(crate) fn register<E>(&mut self) -> Result<(), StateTreeEventRegistrationError>
    where
        E: StateTreeEvent,
    {
        self.0.register_payload::<E>().map(|_| ())
    }

    pub(crate) fn protocol(&self) -> &StateTreeEventRegistry {
        &self.0
    }

    pub(crate) fn ensure<E>(&mut self) -> Result<bool, StateTreeEventRegistrationError>
    where
        E: StateTreeEvent,
    {
        if self.0.type_ids.contains_key(&TypeId::of::<E>()) {
            Ok(false)
        } else {
            self.register::<E>().map(|()| true)
        }
    }
}

impl Default for StateTreeEventRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl StateTreeEventRegistry {
    /// Creates an empty event protocol builder.
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            type_ids: HashMap::new(),
            names: HashMap::new(),
            authored_names: Vec::new(),
        }
    }

    /// Registers one concrete event payload in insertion order.
    #[cfg(test)]
    pub(crate) fn register<E>(&mut self) -> Result<&mut Self, StateTreeEventRegistrationError>
    where
        E: StateTreeEvent,
    {
        self.register_payload::<E>()
    }

    fn register_payload<E>(&mut self) -> Result<&mut Self, StateTreeEventRegistrationError>
    where
        E: StateTreeEvent,
    {
        if E::AUTHORED_NAME.is_empty() {
            return Err(StateTreeEventRegistrationError::EmptyName {
                type_name: type_name::<E>(),
            });
        }
        let type_id = TypeId::of::<E>();
        if self.type_ids.contains_key(&type_id) {
            return Err(StateTreeEventRegistrationError::DuplicateType {
                type_name: type_name::<E>(),
            });
        }
        if let Some(registered_type_name) = self.names.get(E::AUTHORED_NAME) {
            return Err(StateTreeEventRegistrationError::DuplicateName {
                name: E::AUTHORED_NAME,
                registered_type_name,
            });
        }

        let id = RegisteredEventId(self.entries.len());
        self.type_ids.insert(type_id, id);
        self.names.insert(E::AUTHORED_NAME, type_name::<E>());
        self.entries.push(RegisteredEvent {
            id,
            type_id,
            authored_name: E::AUTHORED_NAME,
            event_data: project_event_data::<E>,
        });
        self.authored_names.push(E::AUTHORED_NAME);
        Ok(self)
    }
}

pub(crate) fn register_event<E>(world: &mut World, origin: RegistrationOrigin)
where
    E: StateTreeEvent,
{
    let result = world
        .get_resource_or_init::<StateTreeAppEventRegistry>()
        .ensure::<E>();
    match result {
        Ok(false) => {}
        Ok(true) => {
            world
                .get_resource_or_init::<crate::StateTreeProtocolInventory>()
                .add_event(E::AUTHORED_NAME.into(), origin);
            crate::plugin::refresh_event_links(world);
        }
        Err(error) => panic!(
            "failed to register state-tree event `{}`: {error}",
            type_name::<E>()
        ),
    }
}

impl StateTreeEventRegistry {
    pub(crate) fn registered_event_id(&self, type_id: TypeId) -> Option<RegisteredEventId> {
        self.type_ids.get(&type_id).copied()
    }

    pub(crate) fn authored_names(&self) -> &[&'static str] {
        &self.authored_names
    }

    pub(crate) fn entry(&self, id: RegisteredEventId) -> &RegisteredEvent {
        &self.entries[id.index()]
    }

    pub(crate) fn queue<E>(&self, payload: E) -> Option<QueuedStateTreeEvent>
    where
        E: StateTreeEvent,
    {
        let id = self.registered_event_id(TypeId::of::<E>())?;
        Some(QueuedStateTreeEvent::new(id, payload))
    }

    pub(crate) fn event_ref<'a>(
        &'a self,
        event: &'a QueuedStateTreeEvent,
    ) -> StateTreeEventRef<'a> {
        let entry = self.entry(event.id);
        debug_assert_eq!(entry.type_id, event.payload.as_any().type_id());
        StateTreeEventRef {
            entry,
            payload: event.payload.as_any(),
        }
    }

    #[cfg(test)]
    pub(crate) fn borrowed_event<'a, E>(&'a self, event: &'a E) -> Option<StateTreeEventRef<'a>>
    where
        E: StateTreeEvent,
    {
        let id = self.registered_event_id(TypeId::of::<E>())?;
        Some(StateTreeEventRef {
            entry: self.entry(id),
            payload: event,
        })
    }
}

/// A typed borrowed view of one queued state-tree event.
pub struct StateTreeEventRef<'a> {
    entry: &'a RegisteredEvent,
    payload: &'a (dyn Any + Send + Sync),
}

impl Clone for StateTreeEventRef<'_> {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for StateTreeEventRef<'_> {}

impl<'a> StateTreeEventRef<'a> {
    /// Returns the event name used by authored state-tree definitions.
    pub fn authored_name(self) -> &'static str {
        self.entry.authored_name
    }

    /// Borrows the payload when this event contains `T`.
    pub fn payload<T: StateTreeEvent>(self) -> Option<&'a T> {
        self.payload.downcast_ref()
    }

    /// Returns whether this event contains a `T` payload.
    pub fn is<T: StateTreeEvent>(self) -> bool {
        self.payload.is::<T>()
    }

    pub(crate) fn event_data(self) -> Option<LuauValue> {
        (self.entry.event_data)(self.payload)
    }

    pub(crate) fn registered_event_id(self) -> RegisteredEventId {
        self.entry.id
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct RegisteredEventId(usize);

impl RegisteredEventId {
    pub(crate) fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone)]
pub(crate) struct RegisteredEvent {
    id: RegisteredEventId,
    type_id: TypeId,
    authored_name: &'static str,
    event_data: EventDataProjector,
}

/// The ownership strategy is deliberately private. `event_storage` benchmarks
/// show the allocation cost directly; a 16-byte inline prototype increased the
/// queue stride and did not improve end-to-end entity-event throughput.
pub(crate) struct QueuedStateTreeEvent {
    id: RegisteredEventId,
    payload: Box<dyn QueuedPayload>,
}

trait QueuedPayload: Send + Sync {
    fn as_any(&self) -> &(dyn Any + Send + Sync);
    fn activity_scope(&self) -> Option<crate::activity::ErasedActivityToken> {
        None
    }
}

struct PlainPayload<E>(E);
impl<E: StateTreeEvent> QueuedPayload for PlainPayload<E> {
    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        &self.0
    }
}

struct ActivityPayload<E> {
    event: E,
    scope: crate::activity::ErasedActivityToken,
}
impl<E: StateTreeEvent> QueuedPayload for ActivityPayload<E> {
    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        &self.event
    }
    fn activity_scope(&self) -> Option<crate::activity::ErasedActivityToken> {
        Some(self.scope)
    }
}

impl QueuedStateTreeEvent {
    pub(crate) fn new<E: StateTreeEvent>(id: RegisteredEventId, payload: E) -> Self {
        Self {
            id,
            payload: Box::new(PlainPayload(payload)),
        }
    }

    pub(crate) fn for_activity<E: StateTreeEvent>(
        id: RegisteredEventId,
        event: E,
        scope: crate::activity::ErasedActivityToken,
    ) -> Self {
        Self {
            id,
            payload: Box::new(ActivityPayload { event, scope }),
        }
    }

    pub(crate) fn activity_scope(&self) -> Option<crate::activity::ErasedActivityToken> {
        self.payload.activity_scope()
    }

    pub(crate) fn view<'a>(
        &'a self,
        protocol: &'a StateTreeEventRegistry,
    ) -> StateTreeEventRef<'a> {
        protocol.event_ref(self)
    }

    #[cfg(test)]
    pub(crate) fn project<T: StateTreeEvent>(&self) -> Option<&T> {
        self.payload.as_any().downcast_ref()
    }
}

type EventDataProjector = fn(&(dyn Any + Send + Sync)) -> Option<LuauValue>;

fn project_event_data<E>(payload: &(dyn Any + Send + Sync)) -> Option<LuauValue>
where
    E: StateTreeEvent,
{
    let payload = payload
        .downcast_ref::<E>()
        .expect("registered event payload type must match its registry entry");
    payload.event_data()
}

#[cfg(test)]
mod tests {
    use std::{
        any::{TypeId, type_name},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use bevy_ecs::event::Event;

    use super::*;

    #[derive(Event)]
    struct First(u32);

    impl StateTreeEvent for First {
        const AUTHORED_NAME: &'static str = "first";
    }

    #[derive(Event)]
    struct Second(&'static str);

    impl StateTreeEvent for Second {
        const AUTHORED_NAME: &'static str = "second";
    }

    #[derive(Event)]
    struct SameName;

    impl StateTreeEvent for SameName {
        const AUTHORED_NAME: &'static str = "first";
    }

    #[derive(Event)]
    struct Unregistered;

    impl StateTreeEvent for Unregistered {
        const AUTHORED_NAME: &'static str = "unregistered";
    }

    #[derive(Event)]
    struct EmptyName;

    impl StateTreeEvent for EmptyName {
        const AUTHORED_NAME: &'static str = "";
    }

    static PROJECTIONS: AtomicUsize = AtomicUsize::new(0);

    #[derive(Event)]
    struct Projected(i64);

    impl StateTreeEvent for Projected {
        const AUTHORED_NAME: &'static str = "projected";

        fn event_data(&self) -> Option<LuauValue> {
            PROJECTIONS.fetch_add(1, Ordering::Relaxed);
            Some(LuauValue::Integer(self.0))
        }
    }

    #[test]
    fn registration_order_produces_dense_usable_dispatch() {
        let mut registry = StateTreeEventRegistry::new();
        registry
            .register::<First>()
            .unwrap()
            .register::<Second>()
            .unwrap();
        let protocol = registry;

        assert_eq!(protocol.authored_names(), &["first", "second"]);
        assert_eq!(
            protocol.registered_event_id(TypeId::of::<First>()),
            Some(RegisteredEventId(0))
        );
        assert_eq!(
            protocol.registered_event_id(TypeId::of::<Second>()),
            Some(RegisteredEventId(1))
        );

        let first = protocol.queue(First(7)).unwrap();
        let second = protocol.queue(Second("payload")).unwrap();
        let first = first.view(&protocol);
        let second = second.view(&protocol);
        assert_eq!(first.registered_event_id().index(), 0);
        assert_eq!(first.payload::<First>().unwrap().0, 7);
        assert_eq!(second.registered_event_id().index(), 1);
        assert_eq!(second.payload::<Second>().unwrap().0, "payload");
    }

    #[test]
    fn duplicate_rust_type_is_rejected() {
        let mut registry = StateTreeEventRegistry::new();
        registry.register::<First>().unwrap();

        assert_eq!(
            registry.register::<First>().err().unwrap(),
            StateTreeEventRegistrationError::DuplicateType {
                type_name: type_name::<First>(),
            }
        );
    }

    #[test]
    fn duplicate_authored_names_are_rejected() {
        let mut registry = StateTreeEventRegistry::new();
        registry.register::<First>().unwrap();

        assert_eq!(
            registry.register::<SameName>().err().unwrap(),
            StateTreeEventRegistrationError::DuplicateName {
                name: "first",
                registered_type_name: type_name::<First>(),
            }
        );
    }

    #[test]
    fn ensuring_the_same_event_type_is_idempotent() {
        let mut registry = StateTreeAppEventRegistry::default();
        registry.ensure::<First>().unwrap();
        registry.ensure::<First>().unwrap();

        assert_eq!(registry.protocol().authored_names(), &["first"]);
    }

    #[test]
    fn additions_preserve_queued_events_and_existing_ids() {
        let mut registry = StateTreeEventRegistry::new();
        registry.register::<First>().unwrap();
        let queued = registry.queue(First(42)).unwrap();
        registry.register::<Second>().unwrap();
        assert_eq!(queued.view(&registry).payload::<First>().unwrap().0, 42);
        assert_eq!(
            queued.view(&registry).registered_event_id(),
            RegisteredEventId(0)
        );
        assert_eq!(
            registry
                .queue(Second("new"))
                .unwrap()
                .view(&registry)
                .registered_event_id(),
            RegisteredEventId(1)
        );
    }

    #[test]
    fn empty_manual_authored_name_is_rejected() {
        let mut registry = StateTreeEventRegistry::new();

        assert_eq!(
            registry.register::<EmptyName>().err().unwrap(),
            StateTreeEventRegistrationError::EmptyName {
                type_name: type_name::<EmptyName>(),
            }
        );
    }

    #[test]
    fn borrowed_typed_views_downcast_correctly() {
        let mut registry = StateTreeEventRegistry::new();
        registry
            .register::<First>()
            .unwrap()
            .register::<Second>()
            .unwrap();
        let protocol = registry;
        let payload = First(42);

        let view = protocol.borrowed_event(&payload).unwrap();
        assert_eq!(view.authored_name(), "first");
        assert!(view.is::<First>());
        assert!(!view.is::<Second>());
        assert_eq!(view.payload::<First>().unwrap().0, 42);
        assert!(view.payload::<Second>().is_none());
    }

    #[test]
    fn unregistered_lookup_and_queue_are_rejected() {
        let protocol = StateTreeEventRegistry::new();
        let payload = Unregistered;

        assert_eq!(
            protocol.registered_event_id(TypeId::of::<Unregistered>()),
            None
        );
        assert!(protocol.borrowed_event(&payload).is_none());
        assert!(protocol.queue(payload).is_none());
    }

    #[test]
    fn event_data_projector_is_lazy() {
        PROJECTIONS.store(0, Ordering::Relaxed);
        let mut registry = StateTreeEventRegistry::new();
        registry.register::<Projected>().unwrap();
        let protocol = registry;
        let queued = protocol.queue(Projected(23)).unwrap();
        let view = queued.view(&protocol);

        assert_eq!(PROJECTIONS.load(Ordering::Relaxed), 0);
        assert_eq!(view.authored_name(), "projected");
        assert!(view.is::<Projected>());
        assert_eq!(PROJECTIONS.load(Ordering::Relaxed), 0);
        assert_eq!(view.event_data(), Some(LuauValue::Integer(23)));
        assert_eq!(PROJECTIONS.load(Ordering::Relaxed), 1);
    }
}
