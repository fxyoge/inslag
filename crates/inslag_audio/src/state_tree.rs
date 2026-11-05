use bevy_asset::AssetPath;
use bevy_ecs::prelude::*;
use inslag_state_tree::{
    LuauValue, StateTreeActivity, StateTreeArgs, StateTreeEffectError, StateTreeEventDataError,
};

use crate::mixer::validate_gain;

#[derive(Clone, Debug)]
pub(crate) struct Sound {
    pub src: AssetPath<'static>,
    pub bus: String,
    pub gain: f32,
    pub speed: f32,
    pub priority: u32,
    pub group: Option<String>,
    pub limit: usize,
    pub looping: bool,
}

#[derive(Message)]
pub(crate) struct Play {
    pub owner: Entity,
    pub sound: Sound,
}

pub(crate) struct Playback(pub Sound);

impl StateTreeActivity for Playback {
    type Output = ();
    type Error = String;

    fn error_data(error: &String) -> Result<Option<LuauValue>, StateTreeEventDataError> {
        Ok(Some(LuauValue::String(error.clone())))
    }
}

#[derive(Message)]
pub(crate) struct BusGain {
    pub owner: Entity,
    pub bus: String,
    pub gain: f32,
}

pub(crate) fn sound(
    args: &mut StateTreeArgs,
    activity: bool,
) -> Result<Sound, StateTreeEffectError> {
    let src: String = args.take("src")?;
    let src = AssetPath::try_parse(&src)
        .map_err(|error| StateTreeEffectError::argument("src", error.to_string()))?
        .into_owned();
    if src.path().as_os_str().is_empty() {
        return Err(StateTreeEffectError::argument(
            "src",
            "asset path cannot be empty",
        ));
    }
    let bus: String = args.take_or("bus", "effects".into())?;
    if bus.is_empty() {
        return Err(StateTreeEffectError::argument("bus", "bus cannot be empty"));
    }
    let gain = args.take_or("gain", 1.0_f32)?;
    validate_gain(gain)
        .map_err(|error| StateTreeEffectError::argument("gain", error.to_string()))?;
    let speed = args.take_or("speed", 1.0_f32)?;
    if !speed.is_finite() || speed <= 0.0 {
        return Err(StateTreeEffectError::argument(
            "speed",
            "speed must be finite and positive",
        ));
    }
    let priority = args.take_or("priority", 0_u32)?;
    let group: String = args.take_or("group", String::new())?;
    let limit = args.take_or("limit", 1_u32)?;
    if limit == 0 {
        return Err(StateTreeEffectError::argument(
            "limit",
            "concurrency limit must be positive",
        ));
    }
    let looping = if activity {
        args.take_or("loop", false)?
    } else {
        false
    };
    Ok(Sound {
        src,
        bus,
        gain,
        speed,
        priority,
        group: (!group.is_empty()).then_some(group),
        limit: limit as usize,
        looping,
    })
}
