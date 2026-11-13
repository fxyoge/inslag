use bevy_ecs::prelude::*;

use super::registration::ActivityCollectionRetention;
use super::{ErasedActivityToken, StateTreeActivity, StateTreeActivityToken};

/// One independently identified activity execution.
pub struct ActivityRun<A: StateTreeActivity> {
    token: StateTreeActivityToken<A>,
    pub activity: A,
}

impl<A: StateTreeActivity> ActivityRun<A> {
    pub fn token(&self) -> StateTreeActivityToken<A> {
        self.token
    }
}

/// Authoritative activity executions attached to their owning entity.
#[derive(Component)]
pub struct ActivityRuns<A: StateTreeActivity> {
    runs: Vec<ActivityRun<A>>,
}

impl<A: StateTreeActivity> ActivityRuns<A> {
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, ActivityRun<A>> {
        self.runs.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, ActivityRun<A>> {
        self.runs.iter_mut()
    }

    pub fn get(&self, token: StateTreeActivityToken<A>) -> Option<&A> {
        self.runs
            .iter()
            .find(|run| run.token == token)
            .map(|run| &run.activity)
    }

    pub fn get_mut(&mut self, token: StateTreeActivityToken<A>) -> Option<&mut A> {
        self.runs
            .iter_mut()
            .find(|run| run.token == token)
            .map(|run| &mut run.activity)
    }
}

impl<'a, A: StateTreeActivity> IntoIterator for &'a ActivityRuns<A> {
    type Item = &'a ActivityRun<A>;
    type IntoIter = std::slice::Iter<'a, ActivityRun<A>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, A: StateTreeActivity> IntoIterator for &'a mut ActivityRuns<A> {
    type Item = &'a mut ActivityRun<A>;
    type IntoIter = std::slice::IterMut<'a, ActivityRun<A>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

pub(super) fn insert<A: StateTreeActivity>(
    world: &mut World,
    token: ErasedActivityToken,
    activity: A,
) {
    let Ok(mut entity) = world.get_entity_mut(token.entity) else {
        return;
    };
    let run = ActivityRun {
        token: token.typed(),
        activity,
    };
    if let Some(mut runs) = entity.get_mut::<ActivityRuns<A>>() {
        if !runs.runs.iter().any(|existing| existing.token == run.token) {
            runs.runs.push(run);
        }
    } else {
        entity.insert(ActivityRuns { runs: vec![run] });
    }
}

pub(super) fn remove<A: StateTreeActivity>(
    world: &mut World,
    token: ErasedActivityToken,
    retention: ActivityCollectionRetention,
) {
    let Ok(mut entity) = world.get_entity_mut(token.entity) else {
        return;
    };
    let token = token.typed();
    if !entity
        .get::<ActivityRuns<A>>()
        .is_some_and(|runs| runs.get(token).is_some())
    {
        return;
    }
    let mut runs = entity.get_mut::<ActivityRuns<A>>().unwrap();
    runs.runs.retain(|run| run.token != token);
    if runs.is_empty() && retention == ActivityCollectionRetention::RemoveWhenEmpty {
        entity.remove::<ActivityRuns<A>>();
    }
}
