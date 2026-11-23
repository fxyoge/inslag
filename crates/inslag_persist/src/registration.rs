use crate::{
    Error,
    backend::{Batch, Record, RecordOwner, Role},
    entity,
    identity::Identities,
};
use bevy_app::App;
use bevy_ecs::{
    change_detection::{DetectChanges, MAX_CHANGE_AGE, Tick},
    entity::EntityHashMap,
    entity_disabling::DefaultQueryFilters,
    prelude::*,
    query::{ArchetypeFilter, QueryState},
    system::SystemParam,
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    any::{Any, TypeId},
    collections::HashSet,
    marker::PhantomData,
    sync::Mutex,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Capture {
    #[default]
    Snapshot,
    /// Save components changed since the last successful save. Scans all matches.
    Changed,
    /// Call [`Dirty::mark`] after editing a component. Inserts and removals are automatic.
    Dirty,
}

pub struct ComponentSave<T, F = ()> {
    key: &'static str,
    capture: Capture,
    attached: bool,
    marker: PhantomData<fn() -> (T, F)>,
}

pub fn component<T: Component + Serialize + DeserializeOwned>(
    key: &'static str,
) -> ComponentSave<T> {
    ComponentSave {
        key,
        capture: Capture::Snapshot,
        attached: false,
        marker: PhantomData,
    }
}

impl<T, F> ComponentSave<T, F> {
    /// Save this component only on entities selected by other registrations.
    pub fn attached(mut self) -> Self {
        self.attached = true;
        self
    }
    pub fn capture(mut self, capture: Capture) -> Self {
        self.capture = capture;
        self
    }

    /// Save only entities matching this filter.
    ///
    /// Save any filter marker components too if the selection should survive
    /// loading. Filtering scans entities even with [`Capture::Dirty`].
    pub fn filter<G: ArchetypeFilter + 'static>(self) -> ComponentSave<T, G> {
        ComponentSave {
            key: self.key,
            capture: self.capture,
            attached: self.attached,
            marker: PhantomData,
        }
    }
}

struct ResourceSave<T> {
    key: &'static str,
    initialize: fn(&mut World),
    marker: PhantomData<T>,
}

fn resource<T: Resource + FromWorld + Serialize + DeserializeOwned>(
    key: &'static str,
) -> ResourceSave<T> {
    ResourceSave {
        key,
        initialize: |world| {
            world.init_resource::<T>();
        },
        marker: PhantomData,
    }
}

pub struct Registration {
    key: &'static str,
    role: Role,
    type_id: TypeId,
    pub(crate) setup: Setup,
}

impl Registration {
    pub(crate) fn new_adapter(key: &'static str, type_id: TypeId, setup: Setup) -> Self {
        Self {
            key,
            role: Role::Global,
            type_id,
            setup,
        }
    }
}

pub(crate) type Setup = Box<dyn FnOnce(&mut World) -> Box<dyn Codec> + Send + Sync>;

pub trait AppSaveExt {
    /// Register a type before the first save or load.
    ///
    /// Keep its key stable: existing saves use it to identify the type.
    fn persist(&mut self, registration: impl Into<Registration>) -> &mut Self;
    fn persist_resource<T: Resource + FromWorld + Serialize + DeserializeOwned>(
        &mut self,
        key: &str,
    ) -> &mut Self;
    fn persist_component<T: Component + Serialize + DeserializeOwned>(
        &mut self,
        key: &str,
    ) -> &mut Self;
    fn persist_attached_component<T: Component + Serialize + DeserializeOwned>(
        &mut self,
        key: &str,
    ) -> &mut Self;
}

impl AppSaveExt for App {
    fn persist_resource<T: Resource + FromWorld + Serialize + DeserializeOwned>(
        &mut self,
        key: &str,
    ) -> &mut Self {
        self.persist(resource::<T>(intern_key(key)))
    }
    fn persist_component<T: Component + Serialize + DeserializeOwned>(
        &mut self,
        key: &str,
    ) -> &mut Self {
        self.persist(component::<T>(intern_key(key)))
    }
    fn persist_attached_component<T: Component + Serialize + DeserializeOwned>(
        &mut self,
        key: &str,
    ) -> &mut Self {
        self.persist(component::<T>(intern_key(key)).attached())
    }
    fn persist(&mut self, registration: impl Into<Registration>) -> &mut Self {
        let registration = registration.into();
        self.init_resource::<Registry>()
            .init_resource::<Identities>();
        let registry = self.world().resource::<Registry>();
        assert!(
            !registry.started,
            "register persistence before the first save/load"
        );
        assert!(
            !registration.key.is_empty(),
            "persistent type keys must not be empty"
        );
        assert!(
            registry
                .entries
                .iter()
                .all(|entry| entry.key != registration.key),
            "duplicate persistent type key: {}",
            registration.key
        );
        assert!(
            registry
                .entries
                .iter()
                .all(|entry| (entry.type_id, entry.role)
                    != (registration.type_id, registration.role)),
            "a type can only be persisted once per component/resource role"
        );
        let codec = (registration.setup)(self.world_mut());
        self.world_mut()
            .resource_mut::<Registry>()
            .entries
            .push(Entry {
                key: registration.key,
                role: registration.role,
                type_id: registration.type_id,
                codec,
            });
        self.world_mut()
            .resource_mut::<Registry>()
            .entries
            .sort_by_key(|entry| entry.codec.priority());
        self
    }
}

#[derive(SystemParam)]
pub struct Dirty<'w, T: Component> {
    // Shared scheduler access; the mutex protects only the pending set.
    tracking: Res<'w, Tracking<T>>,
}

impl<T: Component> Dirty<'_, T> {
    pub fn mark(&mut self, entity: Entity) {
        self.tracking
            .pending
            .lock()
            .expect("dirty queue poisoned")
            .insert(entity);
    }
}

#[derive(Resource)]
struct Tracking<T: Component> {
    members: HashSet<Entity>,
    pending: Mutex<HashSet<Entity>>,
    filtered: bool,
    marker: PhantomData<T>,
}

fn inserted<T: Component>(
    event: On<Insert<T>>,
    mut tracking: ResMut<Tracking<T>>,
    mut ids: ResMut<Identities>,
) {
    if ids.suspended {
        return;
    }
    if !tracking.filtered && tracking.members.insert(event.entity) {
        ids.enter(event.entity);
    }
    tracking
        .pending
        .get_mut()
        .expect("dirty queue poisoned")
        .insert(event.entity);
}

fn removed<T: Component>(
    event: On<Remove<T>>,
    mut tracking: ResMut<Tracking<T>>,
    mut ids: ResMut<Identities>,
) {
    if ids.suspended {
        return;
    }
    if tracking.members.remove(&event.entity) {
        ids.leave(event.entity);
    }
    tracking
        .pending
        .get_mut()
        .expect("dirty queue poisoned")
        .insert(event.entity);
}

#[derive(Resource, Default)]
pub(crate) struct Registry {
    pub entries: Vec<Entry>,
    pub baseline: bool,
    pub started: bool,
    pub generation: Option<u64>,
    pub rewrite_manifest: bool,
}

pub(crate) struct Entry {
    pub key: &'static str,
    pub role: Role,
    type_id: TypeId,
    pub codec: Box<dyn Codec>,
}

pub(crate) trait Codec: Send + Sync {
    fn priority(&self) -> u8 {
        1
    }
    fn initialize(&self, _world: &mut World) {}
    fn clear(&self, _world: &mut World) {}
    fn reconcile(&mut self, world: &mut World);
    fn capture(
        &mut self,
        world: &mut World,
        baseline: bool,
        batch: &mut Batch,
    ) -> Result<Option<Box<dyn Any>>, Error>;
    fn decode(
        &self,
        world: &mut World,
        records: &[(RecordOwner, Vec<u8>)],
    ) -> Result<Box<dyn Decoded>, Error>;
    fn decode_missing(&self, world: &mut World) -> Result<Box<dyn Decoded>, Error> {
        self.decode(world, &[])
    }
    fn begin_commit(&mut self, _world: &mut World) {}
    fn finish_commit(&mut self, _world: &mut World, _tick: Tick, _committed: bool) {}
    fn reset(&mut self, world: &mut World);
}

pub(crate) trait Decoded {
    fn entities(&self, _: &std::collections::HashMap<i64, Entity>) -> Vec<Entity> {
        Vec::new()
    }
    fn apply(
        self: Box<Self>,
        world: &mut World,
        ids: &std::collections::HashMap<i64, Entity>,
        mapper: &mut EntityHashMap<Entity>,
    );
}

struct ComponentCodec<T: Component, F: ArchetypeFilter + 'static> {
    key: &'static str,
    capture: Capture,
    query: QueryState<(Entity, Ref<'static, T>), F>,
    last_commit: Tick,
    committing: HashSet<Entity>,
    attached: bool,
    attachment_matches: Option<AttachmentMatches>,
}

#[derive(Default)]
struct AttachmentMatches {
    committed: HashSet<(Entity, i64)>,
    captured: HashSet<(Entity, i64)>,
}

impl<T, F> From<ComponentSave<T, F>> for Registration
where
    T: Component + Serialize + DeserializeOwned,
    F: ArchetypeFilter + 'static,
{
    fn from(value: ComponentSave<T, F>) -> Self {
        Self {
            key: value.key,
            role: Role::Component,
            type_id: TypeId::of::<T>(),
            setup: Box::new(move |world| {
                let filtered = TypeId::of::<F>() != TypeId::of::<()>();
                let attachment_matches =
                    (value.attached && filtered).then(AttachmentMatches::default);
                world.insert_resource(Tracking::<T> {
                    members: HashSet::new(),
                    pending: Mutex::default(),
                    filtered: value.attached || filtered,
                    marker: PhantomData,
                });
                if attachment_matches.is_none() {
                    world.add_observer(inserted::<T>);
                    world.add_observer(removed::<T>);
                }
                let mut codec = ComponentCodec::<T, F> {
                    key: value.key,
                    capture: value.capture,
                    query: if world.contains_resource::<DefaultQueryFilters>() {
                        world.resource_scope(|world, _: Mut<DefaultQueryFilters>| {
                            world.query_filtered()
                        })
                    } else {
                        world.query_filtered()
                    },
                    last_commit: Tick::new(0),
                    committing: HashSet::new(),
                    attached: value.attached,
                    attachment_matches,
                };
                codec.reset(world);
                Box::new(codec)
            }),
        }
    }
}

impl<T, F> ComponentCodec<T, F>
where
    T: Component + Serialize + DeserializeOwned,
    F: ArchetypeFilter + 'static,
{
    fn refresh_membership(&mut self, world: &mut World) {
        if self.attached {
            return;
        }
        self.query.update_archetypes(world);
        let ordered: Vec<_> = self.query.iter(world).map(|(entity, _)| entity).collect();
        let selected: HashSet<_> = ordered.iter().copied().collect();
        world.resource_scope(|world, mut tracking: Mut<Tracking<T>>| {
            let mut ids = world.resource_mut::<Identities>();
            for entity in tracking.members.difference(&selected) {
                ids.leave(*entity);
                tracking
                    .pending
                    .lock()
                    .expect("dirty queue poisoned")
                    .insert(*entity);
            }
            // Allocate initial IDs in query order, not randomized hash order.
            // Preserve storage locality for components present at registration.
            for entity in ordered
                .iter()
                .filter(|entity| !tracking.members.contains(entity))
            {
                ids.enter(*entity);
                tracking
                    .pending
                    .lock()
                    .expect("dirty queue poisoned")
                    .insert(*entity);
            }
            tracking.members = selected;
        });
    }

    fn encode(
        &self,
        entity: Entity,
        value: &T,
        world: &World,
        batch: &mut Batch,
    ) -> Result<(), Error> {
        let id = world.resource::<Identities>().entries[&entity].id;
        let (payload, references) = entity::references(|| serde_json::to_vec(value));
        let payload = payload.map_err(|source| Error::Encode {
            key: self.key,
            source,
        })?;
        batch.upserts.push(Record {
            owner: RecordOwner::entity(id),
            key: self.key,
            payload,
            references,
        });
        Ok(())
    }
}

impl<T, F> Codec for ComponentCodec<T, F>
where
    T: Component + Serialize + DeserializeOwned,
    F: ArchetypeFilter + 'static,
{
    fn reconcile(&mut self, world: &mut World) {
        self.query.update_archetypes(world);
        if world.resource::<Tracking<T>>().filtered {
            self.refresh_membership(world);
        }
    }

    fn capture(
        &mut self,
        world: &mut World,
        baseline: bool,
        batch: &mut Batch,
    ) -> Result<Option<Box<dyn Any>>, Error> {
        if self.attached {
            let ids = world.resource::<Identities>();
            if let Some(matches) = &mut self.attachment_matches {
                matches.captured.clear();
            }
            for (entity, value) in self.query.iter_manual(world) {
                if let Some(identity) = ids.entries.get(&entity).filter(|id| id.memberships > 0) {
                    self.encode(entity, &value, world, batch)?;
                    if let Some(matches) = &mut self.attachment_matches {
                        matches.captured.insert((entity, identity.id));
                    }
                }
            }
            if let Some(matches) = &self.attachment_matches {
                batch.deletes.extend(
                    matches
                        .committed
                        .difference(&matches.captured)
                        // Keep deletion counts consistent with unfiltered attachments:
                        // losing ownership alone is handled by the entity cascade.
                        .filter(|(entity, _)| {
                            !ids.removed.contains(entity) || world.get::<T>(*entity).is_none()
                        })
                        .map(|&(_, id)| (RecordOwner::entity(id), self.key)),
                );
            } else {
                let tracking = world.resource::<Tracking<T>>();
                let pending = tracking.pending.lock().expect("dirty queue poisoned");
                for entity in pending
                    .iter()
                    .filter(|entity| world.get::<T>(**entity).is_none())
                {
                    if let Some(identity) = ids.entries.get(entity) {
                        batch
                            .deletes
                            .push((RecordOwner::entity(identity.id), self.key));
                    }
                }
            }
            return Ok(None);
        }
        let tracking = world.resource::<Tracking<T>>();
        let pending = tracking.pending.lock().expect("dirty queue poisoned");
        let expired = world
            .read_change_tick()
            .get()
            .wrapping_sub(self.last_commit.get())
            >= MAX_CHANGE_AGE;
        if self.capture == Capture::Dirty && baseline {
            for entity in pending
                .iter()
                .filter(|entity| tracking.members.contains(entity))
            {
                if let Some(value) = world.get::<T>(*entity) {
                    self.encode(*entity, value, world, batch)?;
                }
            }
        } else {
            for (entity, value) in self.query.iter_manual(world) {
                if !baseline
                    || self.capture == Capture::Snapshot
                    || expired
                    || pending.contains(&entity)
                    || value
                        .last_changed()
                        .is_newer_than(self.last_commit, world.read_change_tick())
                {
                    self.encode(entity, &value, world, batch)?;
                }
            }
        }
        let ids = world.resource::<Identities>();
        for entity in pending
            .iter()
            .filter(|entity| !tracking.members.contains(entity))
        {
            if let Some(identity) = ids.entries.get(entity) {
                batch
                    .deletes
                    .push((RecordOwner::entity(identity.id), self.key));
            }
        }
        Ok(None)
    }

    fn decode(
        &self,
        _world: &mut World,
        records: &[(RecordOwner, Vec<u8>)],
    ) -> Result<Box<dyn Decoded>, Error> {
        let values = records
            .iter()
            .map(|(owner, payload)| {
                let id = owner
                    .entity_id()
                    .ok_or_else(|| Error::InvalidData("component stored as a resource".into()))?;
                serde_json::from_slice(payload)
                    .map(|value| (id, value))
                    .map_err(|source| Error::Decode {
                        key: self.key,
                        source,
                    })
            })
            .collect::<Result<Vec<(i64, T)>, _>>()?;
        Ok(Box::new(DecodedComponent(values)))
    }

    fn begin_commit(&mut self, world: &mut World) {
        let mut tracking = world.resource_mut::<Tracking<T>>();
        let pending = tracking.pending.get_mut().expect("dirty queue poisoned");
        std::mem::swap(pending, &mut self.committing);
    }

    fn finish_commit(&mut self, world: &mut World, tick: Tick, committed: bool) {
        if committed {
            self.last_commit = tick;
            self.committing.clear();
            if let Some(matches) = &mut self.attachment_matches {
                std::mem::swap(&mut matches.committed, &mut matches.captured);
            }
        } else {
            world
                .resource_mut::<Tracking<T>>()
                .pending
                .get_mut()
                .expect("dirty queue poisoned")
                .extend(self.committing.drain());
        }
    }

    fn reset(&mut self, world: &mut World) {
        if let Some(matches) = &mut self.attachment_matches {
            matches.committed.clear();
            matches.captured.clear();
        }
        world.resource_mut::<Tracking<T>>().members.clear();
        world
            .resource_mut::<Tracking<T>>()
            .pending
            .get_mut()
            .expect("dirty queue poisoned")
            .clear();
        self.refresh_membership(world);
    }
}

struct DecodedComponent<T>(Vec<(i64, T)>);

impl<T: Component> Decoded for DecodedComponent<T> {
    fn entities(&self, ids: &std::collections::HashMap<i64, Entity>) -> Vec<Entity> {
        self.0.iter().map(|(id, _)| ids[id]).collect()
    }
    fn apply(
        self: Box<Self>,
        world: &mut World,
        ids: &std::collections::HashMap<i64, Entity>,
        mapper: &mut EntityHashMap<Entity>,
    ) {
        for (id, mut value) in self.0 {
            T::map_entities(&mut value, mapper);
            world.entity_mut(ids[&id]).insert(value);
        }
    }
}

struct ResourceCodec<T> {
    key: &'static str,
    present: bool,
    captured_present: bool,
    initialize: fn(&mut World),
    marker: PhantomData<T>,
}

impl<T: Resource + Serialize + DeserializeOwned> From<ResourceSave<T>> for Registration {
    fn from(value: ResourceSave<T>) -> Self {
        Self {
            key: value.key,
            role: Role::Global,
            type_id: TypeId::of::<T>(),
            setup: Box::new(move |_| {
                Box::new(ResourceCodec::<T> {
                    key: value.key,
                    present: false,
                    captured_present: false,
                    initialize: value.initialize,
                    marker: PhantomData,
                })
            }),
        }
    }
}

impl<T: Resource + Serialize + DeserializeOwned> Codec for ResourceCodec<T> {
    fn decode_missing(&self, _world: &mut World) -> Result<Box<dyn Decoded>, Error> {
        Ok(Box::new(InitializeResource(self.initialize)))
    }
    fn initialize(&self, world: &mut World) {
        (self.initialize)(world);
    }
    fn clear(&self, world: &mut World) {
        world.remove_resource::<T>();
    }
    fn reconcile(&mut self, _: &mut World) {}

    fn capture(
        &mut self,
        world: &mut World,
        baseline: bool,
        batch: &mut Batch,
    ) -> Result<Option<Box<dyn Any>>, Error> {
        if let Some(value) = world.get_resource::<T>() {
            let encode = || {
                serde_json::to_vec(value).map_err(|source| Error::Encode {
                    key: self.key,
                    source,
                })
            };
            let (payload, references) = entity::references(encode);
            batch.upserts.push(Record {
                owner: RecordOwner::GLOBAL,
                key: self.key,
                payload: payload?,
                references,
            });
        } else if baseline && self.present {
            batch.deletes.push((RecordOwner::GLOBAL, self.key));
        }
        Ok(None)
    }

    fn decode(
        &self,
        _world: &mut World,
        records: &[(RecordOwner, Vec<u8>)],
    ) -> Result<Box<dyn Decoded>, Error> {
        if records.len() > 1
            || records
                .first()
                .is_some_and(|(owner, _)| *owner != RecordOwner::GLOBAL)
        {
            return Err(Error::InvalidData("resource stored as components".into()));
        }
        let value = records
            .first()
            .map(|(_, payload)| {
                let decode = || {
                    serde_json::from_slice(payload).map_err(|source| Error::Decode {
                        key: self.key,
                        source,
                    })
                };
                decode()
            })
            .transpose()?;
        Ok(Box::new(DecodedResource::<T> { value }))
    }

    fn begin_commit(&mut self, world: &mut World) {
        self.captured_present = world.contains_resource::<T>();
    }
    fn finish_commit(&mut self, _: &mut World, _: Tick, committed: bool) {
        if committed {
            self.present = self.captured_present;
        }
    }
    fn reset(&mut self, _: &mut World) {
        self.present = false;
    }
}

pub(crate) fn intern_key(key: &str) -> &'static str {
    use std::sync::OnceLock;
    static KEYS: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut keys = KEYS
        .get_or_init(Default::default)
        .lock()
        .expect("save key interner poisoned");
    if let Some(key) = keys.get(key) {
        return key;
    }
    let key = Box::leak(key.to_owned().into_boxed_str());
    keys.insert(key);
    key
}

#[doc(hidden)]
pub struct Declaration {
    pub key: &'static str,
    pub install: fn(&mut App),
}
inventory::collect!(Declaration);

#[doc(hidden)]
pub fn declare_resource<T: Resource + FromWorld + Serialize + DeserializeOwned>(
    app: &mut App,
    key: &str,
) {
    app.persist_resource::<T>(key);
}

#[doc(hidden)]
pub fn declare_component<T: Component + Serialize + DeserializeOwned>(app: &mut App, key: &str) {
    app.persist_component::<T>(key);
}

pub(crate) fn initialize(world: &mut World) {
    world.resource_scope(|world, registry: Mut<Registry>| {
        for entry in &registry.entries {
            entry.codec.initialize(world);
        }
    });
}

struct DecodedResource<T> {
    value: Option<T>,
}

struct InitializeResource(fn(&mut World));

impl Decoded for InitializeResource {
    fn apply(
        self: Box<Self>,
        world: &mut World,
        _: &std::collections::HashMap<i64, Entity>,
        _: &mut EntityHashMap<Entity>,
    ) {
        (self.0)(world);
    }
}

impl<T: Resource> Decoded for DecodedResource<T> {
    fn apply(
        self: Box<Self>,
        world: &mut World,
        _: &std::collections::HashMap<i64, Entity>,
        _: &mut EntityHashMap<Entity>,
    ) {
        if let Some(value) = self.value {
            world.insert_resource(value);
        } else {
            world.remove_resource::<T>();
        }
    }
}
