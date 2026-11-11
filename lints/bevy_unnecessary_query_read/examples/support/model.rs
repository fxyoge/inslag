use bevy_ecs::prelude::*;

#[derive(Component)]
pub struct Health(pub u32);

#[derive(Component)]
pub struct Shield;

#[derive(Component)]
pub struct Hidden;
