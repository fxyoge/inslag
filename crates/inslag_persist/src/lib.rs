//! Save Bevy resources and components to SQLite.
//!
//! Mark types with [`#[save]`](save), add [`SavePlugin`], and send a
//! [`SaveRequest`] when you want to save.
//!
//! # Save resources and components
//!
//! ```no_run
//! # #[cfg(feature = "sqlite")]
//! # {
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
//! app.add_plugins(SavePlugin::sqlite("saves"));
//! app.world_mut().write_message(SaveRequest::New);
//! app.update();
//!
//! app.world_mut().resource_mut::<Score>().0 = 10;
//! app.world_mut().spawn(Health(100));
//! app.world_mut().write_message(SaveRequest::Save);
//! app.update();
//! # }
//! ```
//!
//! For a new save, resources are initialized with [`FromWorld`] or [`Default`].
//! Entities with saved components are included; their [`Name`] and
//! [`Transform`](bevy_transform::components::Transform) components are saved too.
//!
//! Give each saved type a unique key and keep it stable when renaming Rust types.
//! Check [`SaveCompleted`] for load and save results.
//!
//! # Autosave
//!
//! Autosave is off by default. Install [`TimePlugin`](bevy_time::TimePlugin)
//! before enabling it:
//!
//! ```no_run
//! # #[cfg(feature = "sqlite")]
//! # {
//! use bevy_app::App;
//! use bevy_time::TimePlugin;
//! use inslag_persist::SavePlugin;
//! use std::time::Duration;
//!
//! let mut app = App::new();
//! app.add_plugins(TimePlugin)
//!     .add_plugins(SavePlugin::sqlite("saves").autosave(Duration::from_secs(30)));
//! # }
//! ```
//!
//! The interval uses real time, so pausing the simulation does not pause autosave.
mod plugins;
pub use plugins::SavePlugin;

pub mod adapter;
#[cfg_attr(
    not(feature = "sqlite"),
    allow(dead_code, reason = "no backend enabled")
)]
mod backend;
mod identity;
mod lifecycle;
mod registration;
mod storage;
pub use inslag_persist_macros::save;
extern crate self as inslag_persist;

pub mod asset;
pub mod entity;

pub use lifecycle::{
    LoadOutcome, PrepareSave, SaveClosed, SaveCompleted, SaveOpened, SaveRequest, SaveSet,
    SaveSettings, SaveStatus,
};
pub use registration::{AppSaveExt, Capture, ComponentSave, Dirty, Registration, component};
pub use storage::{Error, SaveStats};

#[doc(hidden)]
pub mod __private {
    pub use crate::registration::{Declaration, declare_component, declare_resource};
    pub use inventory;
}

pub mod prelude {
    pub use crate::{
        SaveCompleted, SaveHandle, SaveInfo, SavePlugin, SaveRequest, SaveSettings, SaveStatus,
        save,
    };
}

use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::SystemCondition;
use std::time::{Duration, SystemTime};

type ConfigureReadiness = Box<dyn Fn(&mut App) + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SaveHandle(u128);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveInfo {
    pub handle: SaveHandle,
    pub created_at: SystemTime,
}
