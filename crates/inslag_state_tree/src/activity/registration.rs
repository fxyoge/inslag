use std::{any::TypeId, collections::HashSet, marker::PhantomData};

use bevy_ecs::prelude::*;

use super::{ErasedActivityToken, StateTreeActivity};

/// How collection storage behaves after its final run stops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActivityCollectionRetention {
    #[default]
    RemoveWhenEmpty,
    KeepEmpty,
}

/// Composable ECS bindings for one authored activity name.
pub struct Activity<A: StateTreeActivity> {
    pub(super) name: Box<str>,
    pub(super) markers: Vec<MarkerBinding>,
    pub(super) collection: bool,
    pub(super) retention: ActivityCollectionRetention,
    pub(super) clone_activity: Option<fn(&A) -> A>,
    pub(super) lifecycle: bool,
}

impl<A: StateTreeActivity> Activity<A> {
    pub fn new(name: impl Into<Box<str>>) -> Self {
        Self {
            name: name.into(),
            markers: Vec::new(),
            collection: false,
            retention: ActivityCollectionRetention::default(),
            clone_activity: None,
            lifecycle: false,
        }
    }

    /// Keeps `C` present while any registered activity contributes it.
    /// The framework owns removal of this marker.
    pub fn with_marker<C: Component + Default>(mut self) -> Self {
        if !self
            .markers
            .iter()
            .any(|marker| marker.id == TypeId::of::<C>())
        {
            self.markers.push(MarkerBinding {
                id: TypeId::of::<C>(),
                start: start_marker::<C>,
                stop: stop_marker::<C>,
            });
        }
        self
    }

    /// Exposes independent runs through [`super::ActivityRuns`] on their owner.
    /// Payloads are cloned only when lifecycle messages also need ownership.
    pub fn with_collection(mut self) -> Self
    where
        A: Clone,
    {
        self.collection = true;
        self.clone_activity = Some(A::clone);
        self
    }

    pub fn with_collection_retention(mut self, retention: ActivityCollectionRetention) -> Self
    where
        A: Clone,
    {
        self = self.with_collection();
        self.retention = retention;
        self
    }

    /// Enables `MessageReader<StateTreeActivityLifecycle<A>>` for custom systems.
    /// Register readers with `add_systems` and choose their ordering normally.
    /// A delayed start message may name an inactive token; check it before
    /// mounting live state. Cancellation messages carry identity, not run data.
    pub fn with_lifecycle(mut self) -> Self {
        self.lifecycle = true;
        self
    }
}

#[derive(Clone, Copy)]
pub(super) struct MarkerBinding {
    id: TypeId,
    pub start: fn(&mut World, ErasedActivityToken),
    pub stop: fn(&mut World, ErasedActivityToken),
}

#[derive(Component)]
struct MarkerContributors<C: Component> {
    tokens: HashSet<ErasedActivityToken>,
    marker: PhantomData<fn() -> C>,
}

fn start_marker<C: Component + Default>(world: &mut World, token: ErasedActivityToken) {
    let Ok(mut entity) = world.get_entity_mut(token.entity) else {
        return;
    };
    if let Some(mut contributors) = entity.get_mut::<MarkerContributors<C>>() {
        contributors.tokens.insert(token);
    } else {
        let contributors = MarkerContributors::<C> {
            tokens: HashSet::from([token]),
            marker: PhantomData,
        };
        if entity.contains::<C>() {
            entity.insert(contributors);
        } else {
            entity.insert((C::default(), contributors));
        }
    }
}

fn stop_marker<C: Component>(world: &mut World, token: ErasedActivityToken) {
    let Ok(mut entity) = world.get_entity_mut(token.entity) else {
        return;
    };
    if !entity
        .get::<MarkerContributors<C>>()
        .is_some_and(|contributors| contributors.tokens.contains(&token))
    {
        return;
    }
    let mut contributors = entity.get_mut::<MarkerContributors<C>>().unwrap();
    contributors.tokens.remove(&token);
    if contributors.tokens.is_empty() {
        entity.remove::<(C, MarkerContributors<C>)>();
    }
}
