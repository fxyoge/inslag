#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Resource)]
struct State {
    game: Game,
}
struct Game {
    tick: u64,
    lanes: Vec<Lane>,
}
struct Lane {
    objects: Vec<i32>,
    speed: i32,
    submerge: Option<u32>,
}
struct Snapshot {
    tick: u64,
    lanes: Vec<LaneSnapshot>,
}
struct LaneSnapshot {
    row: u8,
    submerged: bool,
    spans: Vec<Span>,
}
struct Span {
    x: i32,
    active: bool,
}
impl Lane {
    fn submerged(&self, tick: u64) -> bool {
        self.submerge
            .is_some_and(|period| tick % u64::from(period) < 2)
    }
}
fn visible(lane: &Lane, tick: u64) -> Vec<i32> {
    let mut spans = Vec::new();
    for x in &lane.objects {
        spans.push(*x + lane.speed * tick as i32);
    }
    spans
}
impl Game {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            tick: self.tick,
            lanes: self
                .lanes
                .iter()
                .enumerate()
                .map(|(row, lane)| {
                    let submerged = lane.submerged(self.tick);
                    let spans = visible(lane, self.tick)
                        .into_iter()
                        .map(|x| Span {
                            x,
                            active: !submerged,
                        })
                        .collect();
                    LaneSnapshot {
                        row: row as u8,
                        submerged,
                        spans,
                    }
                })
                .collect(),
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.game.snapshot().tick
}
fn main() {}
