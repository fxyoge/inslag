//! Save Bevy resources and components to SQLite.
//!
//! Mark types with [`#[save]`](save), add [`SavePlugin`], and send a
//! [`SaveRequest`] when you want to save.
//!
//! # Save resources and components
//!
//! ```no_run
//! use bevy_app::App;
//! use bevy_ecs::prelude::*;
//! use inslag_persist::{save, SavePlugin, SaveRequest};
//! use serde::{Serialize, Deserialize};
//!
//! #[save(resource, key = "game.score")]
//! #[derive(Resource, Default, Serialize, Deserialize)]
//! struct Score(u64);
//!
//! #[save(component, key = "game.health")]
//! #[derive(Component, Serialize, Deserialize)]
//! struct Health(u32);
//!
//! let mut app = App::new();
//! app.add_plugins(SavePlugin::sqlite("save.sqlite"));
//! app.update();
//!
//! app.world_mut().resource_mut::<Score>().0 = 10;
//! app.world_mut().spawn(Health(100));
//! app.world_mut().write_message(SaveRequest::Save);
//! app.update();
//! ```
//!
//! An existing save loads at startup. For a new save, resources are initialized
//! with [`FromWorld`] or [`Default`]. Entities with saved components are included;
//! their [`Name`] and [`Transform`](bevy_transform::components::Transform)
//! components are saved too.
//!
//! Give each saved type a unique key and keep it stable when renaming Rust types.
//! Check [`SaveStatus`] for load and save results.
//!
//! # Autosave
//!
//! Autosave is off by default. Install [`TimePlugin`](bevy_time::TimePlugin)
//! before enabling it:
//!
//! ```no_run
//! use bevy_app::App;
//! use bevy_time::TimePlugin;
//! use inslag_persist::SavePlugin;
//! use std::time::Duration;
//!
//! let mut app = App::new();
//! app.add_plugins(TimePlugin)
//!     .add_plugins(SavePlugin::sqlite("save.sqlite").autosave(Duration::from_secs(30)));
//! ```
//!
//! The interval uses real time, so pausing the simulation does not pause autosave.
//! A final save is also attempted when the app exits with [`AppExit`](bevy_app::AppExit).
//!
//! # Open and close saves
//!
//! Send requests through Bevy's message system:
//!
//! - [`SaveRequest::Open(path)`](SaveRequest::Open) saves the current session and opens another file.
//! - [`SaveRequest::Load`] reloads the current file, discarding unsaved changes.
//! - [`SaveRequest::Close`] saves and closes the current file.
//!
//! Use [`SavePlugin::default()`] to start without opening a file. For setup that
//! needs loaded data, add systems to [`SaveOpened`].
pub mod adapter;
mod checkpoint;
mod identity;
mod lifecycle;
mod registration;
mod storage;
pub use checkpoint::Checkpoint;
pub use inslag_persist_macros::save;
extern crate self as inslag_persist;

pub mod asset;
pub mod entity;

pub use lifecycle::{
    PrepareSave, SaveClosed, SaveOpened, SaveRequest, SaveSet, SaveSettings, SaveStatus,
};
pub use registration::{AppSaveExt, Capture, ComponentSave, Dirty, Registration, component};
pub use storage::{Error, SaveStats};

#[doc(hidden)]
pub mod __private {
    pub use crate::registration::{Declaration, declare_component, declare_resource};
    pub use inventory;
}

pub mod prelude {
    pub use crate::{SavePlugin, SaveRequest, SaveSettings, SaveStatus, save};
}

use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::SystemCondition;
use std::path::{Path, PathBuf};
use std::time::Duration;

type ConfigureReadiness = Box<dyn Fn(&mut App) + Send + Sync>;

#[derive(Default)]
pub struct SavePlugin {
    path: Option<PathBuf>,
    load_on_start: bool,
    interval: Option<Duration>,
    readiness: Option<ConfigureReadiness>,
}

impl SavePlugin {
    /// Open a SQLite save at startup, or start fresh if the file does not exist.
    pub fn sqlite(path: impl AsRef<Path>) -> Self {
        Self {
            path: Some(path.as_ref().to_owned()),
            load_on_start: true,
            interval: None,
            readiness: None,
        }
    }

    /// Load the configured file at startup. Enabled by default.
    ///
    /// Set this to `false` to load it later with [`SaveRequest::Load`].
    pub fn load_on_start(mut self, enabled: bool) -> Self {
        self.load_on_start = enabled;
        self
    }

    /// Save at a real-time interval, even while the simulation is paused.
    ///
    /// Install [`TimePlugin`](bevy_time::TimePlugin) first. The interval must be nonzero.
    pub fn autosave(mut self, interval: Duration) -> Self {
        assert!(!interval.is_zero(), "autosave interval must be positive");
        self.interval = Some(interval);
        self
    }

    /// Allow loading and saving only while this condition is true.
    ///
    /// Use this to wait for assets or other setup. A failed load still blocks
    /// saving, and direct [`WorldSaveExt::save`] calls bypass the condition.
    pub fn enabled_when<M>(
        mut self,
        condition: impl SystemCondition<M> + Clone + Send + Sync + 'static,
    ) -> Self {
        self.readiness = Some(Box::new(move |app| {
            app.add_systems(
                lifecycle::CheckReadiness,
                condition.clone().pipe(lifecycle::set_enabled),
            );
        }));
        self
    }
}

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<identity::Identities>()
            .init_resource::<registration::Registry>()
            .insert_resource(SaveStatus::new(self.path.clone()))
            .insert_resource(storage::Storage::new(self.path.clone().unwrap_or_default()));
        if self.path.is_none() {
            app.world_mut().resource_mut::<storage::Storage>().close();
        }
        lifecycle::install(app, self.load_on_start, self.interval);
        app.persist(component::<bevy_ecs::name::Name>("bevy.name").attached())
            .persist(
                component::<bevy_transform::components::Transform>("bevy.transform").attached(),
            );
        let mut declarations: Vec<_> = inventory::iter::<registration::Declaration>
            .into_iter()
            .collect();
        declarations.sort_by_key(|declaration| declaration.key);
        for declaration in declarations {
            (declaration.install)(app);
        }
        if let Some(readiness) = &self.readiness {
            readiness(app);
        }
    }
}

pub trait WorldSaveExt {
    /// Save immediately and return the write statistics.
    ///
    /// Apply pending system commands first, or send [`SaveRequest::Save`] instead.
    /// An existing file must load successfully before it can be overwritten.
    /// Failed saves keep pending changes for retry.
    fn save(&mut self) -> Result<SaveStats, Error>;

    /// Reload the current file, replacing saved entities and resources.
    ///
    /// Invalid data leaves current values unchanged. Side effects from component
    /// hooks cannot be undone if a hook fails during loading.
    fn load(&mut self) -> Result<LoadOutcome, Error>;
}

/// A missing save is a fresh session, not a load failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadOutcome {
    Missing,
    Loaded,
}

impl WorldSaveExt for World {
    fn save(&mut self) -> Result<SaveStats, Error> {
        let result = storage::save(self);
        if self.contains_resource::<SaveStatus>() {
            lifecycle::record_save(self, &result);
        }
        result
    }

    fn load(&mut self) -> Result<LoadOutcome, Error> {
        let result = storage::load(self);
        if matches!(result, Ok(LoadOutcome::Missing)) {
            storage::clear_scope(self);
        }
        if self.contains_resource::<SaveStatus>() {
            lifecycle::record_load(self, &result);
        }
        result
    }
}
