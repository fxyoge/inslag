use bevy_ecs::prelude::*;

#[derive(Component, Default, PartialEq)]
pub struct Health(pub u32);

impl Health {
    pub fn amount(&self) -> u32 {
        self.0
    }

    pub fn reset(&mut self) {
        self.0 = 0;
    }
}

#[derive(Component)]
pub struct Shield(pub u32);

pub fn show(_: u32) {}
