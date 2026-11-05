use std::{collections::BTreeMap, fmt};

use bevy_ecs::prelude::Resource;

/// A rejected bus name or gain passed to [`AudioMixer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MixerError {
    InvalidGain,
    UnknownBus(String),
    InvalidBus,
}

impl fmt::Display for MixerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGain => f.write_str("gain must be finite and between zero and one"),
            Self::UnknownBus(bus) => write!(f, "unknown audio bus `{bus}`"),
            Self::InvalidBus => f.write_str("bus name must be nonempty and cannot be `master`"),
        }
    }
}

impl std::error::Error for MixerError {}

#[derive(Clone, Debug)]
struct Gain {
    preference: f32,
    authored: f32,
}

impl Default for Gain {
    fn default() -> Self {
        Self {
            preference: 1.0,
            authored: 1.0,
        }
    }
}

/// Set user volume preferences on the app's mixer resource.
///
/// Gains range from zero to one. The `master` gain applies to every bus;
/// `music` and `effects` are registered by default.
#[derive(Resource, Clone, Debug)]
pub struct AudioMixer {
    master: Gain,
    buses: BTreeMap<String, Gain>,
}

impl Default for AudioMixer {
    fn default() -> Self {
        Self {
            master: Gain::default(),
            buses: ["music", "effects"]
                .map(|name| (name.into(), Gain::default()))
                .into(),
        }
    }
}

impl AudioMixer {
    /// Register a nonempty bus name. Registering an existing bus preserves its gains.
    pub fn register_bus(&mut self, name: impl Into<String>) -> Result<(), MixerError> {
        let name = name.into();
        if name.is_empty() || name == "master" {
            return Err(MixerError::InvalidBus);
        }
        self.buses.entry(name).or_default();
        Ok(())
    }

    /// Set a bus's user volume from zero to one, independently of authored gain.
    pub fn set_preference(&mut self, bus: &str, gain: f32) -> Result<(), MixerError> {
        validate_gain(gain)?;
        self.bus_mut(bus)?.preference = gain;
        Ok(())
    }

    /// Read the combined master and bus gain, including preferences and authored gain.
    pub fn gain(&self, bus: &str) -> Result<f32, MixerError> {
        let master = self.master.preference * self.master.authored;
        if bus == "master" {
            return Ok(master);
        }
        let gain = self
            .buses
            .get(bus)
            .ok_or_else(|| MixerError::UnknownBus(bus.into()))?;
        Ok(master * gain.preference * gain.authored)
    }

    pub(crate) fn set_authored(&mut self, bus: &str, gain: f32) -> Result<(), MixerError> {
        validate_gain(gain)?;
        self.bus_mut(bus)?.authored = gain;
        Ok(())
    }

    fn bus_mut(&mut self, bus: &str) -> Result<&mut Gain, MixerError> {
        if bus == "master" {
            return Ok(&mut self.master);
        }
        self.buses
            .get_mut(bus)
            .ok_or_else(|| MixerError::UnknownBus(bus.into()))
    }
}

pub(crate) fn validate_gain(gain: f32) -> Result<(), MixerError> {
    if gain.is_finite() && (0.0..=1.0).contains(&gain) {
        Ok(())
    } else {
        Err(MixerError::InvalidGain)
    }
}
