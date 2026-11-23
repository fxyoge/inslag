use bevy_ecs::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Copy)]
pub(crate) struct Identity {
    pub id: i64,
    pub memberships: usize,
}

#[derive(Resource)]
pub(crate) struct Identities {
    pub entries: Arc<HashMap<Entity, Identity>>,
    pub next: i64,
    pub count: usize,
    pub added: HashSet<Entity>,
    pub removed: HashSet<Entity>,
    pub suspended: bool,
}

pub(crate) struct IdentityChanges {
    added: HashSet<Entity>,
    removed: HashSet<Entity>,
}

impl Default for Identities {
    fn default() -> Self {
        Self {
            entries: Arc::default(),
            next: 1,
            count: 0,
            added: HashSet::new(),
            removed: HashSet::new(),
            suspended: false,
        }
    }
}

impl Identities {
    pub(crate) fn enter(&mut self, entity: Entity) {
        let entries = Arc::make_mut(&mut self.entries);
        let entry = entries.entry(entity).or_insert_with(|| {
            let id = self.next;
            self.next = self
                .next
                .checked_add(1)
                .expect("persistent entity IDs exhausted");
            Identity { id, memberships: 0 }
        });
        if entry.memberships == 0 {
            self.count += 1;
            self.added.insert(entity);
            self.removed.remove(&entity);
        }
        entry.memberships += 1;
    }

    pub(crate) fn leave(&mut self, entity: Entity) {
        let entry = Arc::make_mut(&mut self.entries)
            .get_mut(&entity)
            .expect("tracked identity");
        entry.memberships -= 1;
        if entry.memberships == 0 {
            self.count -= 1;
            self.removed.insert(entity);
            self.added.remove(&entity);
        }
    }

    pub(crate) fn begin_commit(&mut self) -> IdentityChanges {
        IdentityChanges {
            added: std::mem::take(&mut self.added),
            removed: std::mem::take(&mut self.removed),
        }
    }

    pub(crate) fn finish_commit(&mut self, changes: IdentityChanges, committed: bool) {
        for entity in changes.added {
            if !committed && self.entries[&entity].memberships > 0 {
                self.added.insert(entity);
            }
        }
        for entity in changes.removed {
            if self.entries[&entity].memberships == 0 {
                if committed {
                    self.removed.remove(&entity);
                    Arc::make_mut(&mut self.entries).remove(&entity);
                } else {
                    self.removed.insert(entity);
                }
            }
        }
    }
}
