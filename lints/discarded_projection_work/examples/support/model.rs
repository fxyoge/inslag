use bevy_ecs::prelude::*;

#[derive(Resource)]
pub struct State {
    pub game: Game,
}

#[derive(Component)]
pub struct Game {
    pub tick: u64,
    pub frog: Frog,
    pub objects: Vec<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frog {
    pub x: i32,
    pub hopping: bool,
}

#[derive(Debug, PartialEq)]
pub struct Snapshot {
    pub tick: u64,
    pub frog: Frog,
    pub objects: Vec<i32>,
}

impl Game {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            tick: self.tick,
            frog: self.frog,
            objects: self.objects.clone(),
        }
    }
}
