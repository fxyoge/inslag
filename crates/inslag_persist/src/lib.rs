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
mod plugins;
pub use plugins::SavePlugin;

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
