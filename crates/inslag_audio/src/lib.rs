//! Play asset-backed sound effects and music from SCXML statecharts.
//!
//! Use `audio.play` for one-shots and `audio.playback` for playback that lasts
//! until completion or state exit. [`AudioMixer`] controls user volume preferences
//! across named buses.
//!
//! # Install
//!
//! Add [`EnhancedAudioPlugin`] after Bevy's audio plugin and
//! [`StateTreePlugin`](inslag_state_tree::StateTreePlugin), using the same schedule
//! as the state tree. With Bevy's `DefaultPlugins` already installed:
//!
//! ```no_run
//! use bevy_app::{App, Update};
//! use inslag_audio::EnhancedAudioPlugin;
//! use inslag_state_tree::StateTreePlugin;
//!
//! # let mut app = App::new();
//! app.add_plugins((
//!     StateTreePlugin::new(Update),
//!     EnhancedAudioPlugin::new(Update),
//! ));
//! ```
//!
//! WAV and Vorbis are enabled by default. Enable `mp3` or `flac` for additional
//! formats; keep at least one format enabled. Linux builds require ALSA development
//! libraries (`libasound2-dev` on Ubuntu).
//!
//! # Author playback
//!
//! Load this through `inslag_state_tree` and attach its definition to an entity:
//!
//! ```xml
//! <scxml xmlns="http://www.w3.org/2005/07/scxml"
//!        xmlns:ist="urn:inslag-state-tree" datamodel="luau" initial="menu">
//!   <state id="menu">
//!     <onentry>
//!       <ist:effect name="audio.play">
//!         <ist:arg name="src" expr="'audio/confirm.wav'"/>
//!       </ist:effect>
//!     </onentry>
//!     <ist:activity id="music" name="audio.playback">
//!       <ist:arg name="src" expr="'audio/menu.ogg'"/>
//!       <ist:arg name="bus" expr="'music'"/>
//!       <ist:arg name="loop" expr="true"/>
//!     </ist:activity>
//!   </state>
//! </scxml>
//! ```
//!
//! Both accept `src`, `bus` (default `effects`), `gain` (0–1), `speed` (positive),
//! `priority` (higher wins), and an optional `group` with a concurrency `limit`
//! (default 1). Groups are scoped to the owning entity. `loop` is available on
//! activities. A completed activity emits `done.activity.<id>`; failed playback
//! emits `error.activity.<id>` with the reason in `_event.data`.
//!
//! # Set volume
//!
//! `master`, `music`, and `effects` are available immediately. Register additional
//! buses before using them in a chart:
//!
//! ```
//! use inslag_audio::AudioMixer;
//!
//! let mut mixer = AudioMixer::default();
//! mixer.register_bus("ambience")?;
//! mixer.set_preference("master", 0.8)?;
//! mixer.set_preference("ambience", 0.5)?;
//! assert_eq!(mixer.gain("ambience")?, 0.4);
//! # Ok::<(), inslag_audio::MixerError>(())
//! ```
//!
//! Change the app's `AudioMixer` resource for preferences. Charts can set a bus's
//! authored gain through `audio.set_bus_gain` with `bus` and `gain` arguments.
//! Playback multiplies master, bus, and per-sound gains. Tune [`AudioSettings`]
//! for voice budgets and timeouts, and read [`AudioDiagnostic`] messages for
//! playback failures.

mod mixer;
mod runtime;
mod state_tree;

pub use mixer::{AudioMixer, MixerError};
pub use runtime::EnhancedAudioPlugin;

use std::time::Duration;

use bevy_ecs::prelude::*;

/// Configure voice budgets, source caching, and playback timeouts.
#[derive(Resource, Clone, Debug)]
pub struct AudioSettings {
    /// Maximum simultaneous one-shot voices.
    pub max_one_shots: usize,
    /// Maximum queued requests and maximum loading one-shots, each capped separately.
    pub max_pending: usize,
    /// Maximum simultaneous state-owned voices, including loading voices.
    pub max_activities: usize,
    /// Maximum cached audio sources; zero disables caching.
    pub cache_capacity: usize,
    /// Drop one-shots that have waited longer than this before playback starts.
    pub one_shot_max_age: Duration,
    /// Fail a request if its asset has not loaded within this duration.
    pub load_timeout: Duration,
    /// Fail playback if the audio output has not started within this duration.
    pub sink_timeout: Duration,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            max_one_shots: 24,
            max_pending: 64,
            max_activities: 8,
            cache_capacity: 64,
            one_shot_max_age: Duration::from_millis(250),
            load_timeout: Duration::from_secs(10),
            sink_timeout: Duration::from_secs(2),
        }
    }
}

/// Read playback failures with a Bevy `MessageReader<AudioDiagnostic>`.
#[derive(Message, Clone, Debug)]
pub struct AudioDiagnostic {
    pub owner: Entity,
    pub src: String,
    pub reason: String,
}

/// Order systems around audio request collection and playback.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, SystemSet)]
pub enum EnhancedAudioSet {
    Collect,
    Playback,
}
