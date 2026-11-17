use std::time::Duration;

use bevy_ecs::prelude::*;
use bevy_time::Time;

#[derive(Resource, Debug)]
pub struct GameTime {
    game_elapsed_at_load: Duration,
    runtime_elapsed_at_load: Duration,
}

impl GameTime {
    pub fn new(time: &Time) -> Self {
        Self::loaded(Duration::ZERO, time)
    }

    pub fn elapsed(&self, time: &Time) -> Duration {
        self.game_elapsed_at_load
            .checked_add(self.session_elapsed(time))
            .expect("game time exceeds the simulation clock range")
    }

    pub fn session_elapsed(&self, time: &Time) -> Duration {
        time.elapsed()
            .checked_sub(self.runtime_elapsed_at_load)
            .expect("simulation time moved backwards")
    }

    pub(crate) fn loaded(game_elapsed: Duration, time: &Time) -> Self {
        Self {
            game_elapsed_at_load: game_elapsed,
            runtime_elapsed_at_load: time.elapsed(),
        }
    }

    pub(crate) fn sample(&self, time: &Time) -> TimeSample {
        TimeSample {
            runtime: time.elapsed(),
            game: self.elapsed(time),
            session: self.session_elapsed(time),
        }
    }
}

impl FromWorld for GameTime {
    fn from_world(world: &mut World) -> Self {
        Self::new(world.resource::<Time>())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TimeSample {
    pub runtime: Duration,
    pub game: Duration,
    pub session: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_game_excludes_runtime_before_creation() {
        let mut time = Time::default();
        time.advance_by(Duration::from_secs(40));
        let game = GameTime::new(&time);
        assert_eq!(game.elapsed(&time), Duration::ZERO);
        assert_eq!(game.session_elapsed(&time), Duration::ZERO);
        time.advance_by(Duration::from_millis(125));
        assert_eq!(game.elapsed(&time), Duration::from_millis(125));
        assert_eq!(game.session_elapsed(&time), Duration::from_millis(125));
    }

    #[test]
    fn loaded_time_keeps_subsecond_precision_after_centuries() {
        let centuries = Duration::new(100 * 365 * 24 * 60 * 60, 123_456_789);
        let mut time = Time::default();
        time.advance_by(Duration::from_secs(40));
        let game = GameTime::loaded(centuries, &time);
        time.advance_by(Duration::from_nanos(1));
        assert_eq!(game.elapsed(&time), centuries + Duration::from_nanos(1));
        assert_eq!(game.session_elapsed(&time), Duration::from_nanos(1));
    }

    #[test]
    fn maximum_duration_is_not_truncated() {
        let time = Time::default();
        assert_eq!(
            GameTime::loaded(Duration::MAX, &time).elapsed(&time),
            Duration::MAX
        );
    }

    #[test]
    #[should_panic(expected = "game time exceeds the simulation clock range")]
    fn game_time_overflow_cannot_wrap() {
        let mut time = Time::default();
        let game = GameTime::loaded(Duration::MAX, &time);
        time.advance_by(Duration::from_nanos(1));
        game.elapsed(&time);
    }
}
