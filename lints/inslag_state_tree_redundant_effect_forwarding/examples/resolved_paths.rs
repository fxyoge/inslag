#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt as Registration;

type Protocol = World;

mod messages {
    use bevy_ecs::prelude::*;

    #[derive(Message)]
    pub enum Command {
        Start,
        Pause,
    }
}

use messages::Command as Renamed;
type Command = Renamed;

fn configure(config: &mut Protocol) {
    Registration::add_state_tree_effect::<Command>(config, "arcade.start");
    Protocol::add_state_tree_effect::<Renamed>(config, "arcade.pause");
}

impl state_tree::StateTreeInput for Command {
    fn from_state_tree_input(
        context: &state_tree::StateTreeContext<'_>,
        _: &mut state_tree::StateTreeArgs,
    ) -> Result<Self, state_tree::StateTreeEffectError> {
        match context.effect_name() {
            Some("arcade.start") => {
                let command = Command::Start;
                Ok(command)
            }
            Some("arcade.pause") => Ok(Renamed::Pause),
            _ => unreachable!(),
        }
    }
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command {
            Renamed::Start => {}
            messages::Command::Pause => {}
        }
    }
}

fn main() {}
