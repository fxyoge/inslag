use bevy_asset::AssetPath;
use bevy_ecs::prelude::*;
use inslag_state_tree::{LuauValue, StateTreeActivity, StateTreeInput};

#[derive(Clone, Debug, StateTreeInput)]
pub(crate) struct Sound {
    pub src: AssetPath<'static>,
    #[state_tree(default = "effects".into())]
    pub bus: String,
    #[state_tree(default = 1.)]
    pub gain: f32,
    #[state_tree(default = 1.)]
    pub speed: f32,
    #[state_tree(default)]
    pub priority: u32,
    pub group: Option<String>,
    #[state_tree(default = 1)]
    pub limit: usize,
}

#[derive(Message, StateTreeInput)]
pub(crate) struct Play {
    #[state_tree(owner)]
    pub owner: Entity,
    #[state_tree(flatten)]
    pub sound: Sound,
}

#[derive(StateTreeInput)]
pub(crate) struct Playback {
    #[state_tree(flatten)]
    pub sound: Sound,
    #[state_tree(rename = "loop", default)]
    pub looping: bool,
}

impl StateTreeActivity for Playback {
    type Output = ();
    type Error = String;

    fn error_data(error: &String) -> Option<LuauValue> {
        Some(LuauValue::String(error.clone()))
    }
}

#[derive(Message, StateTreeInput)]
pub(crate) struct BusGain {
    #[state_tree(owner)]
    pub owner: Entity,
    pub bus: String,
    pub gain: f32,
}
