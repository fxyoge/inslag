use bevy_app::{App, Last};
use inslag_persist::{Error, LoadOutcome, SaveRequest, SaveStats, SaveStatus};
use std::time::{Duration, Instant};

#[allow(
    dead_code,
    reason = "Shared by test targets that exercise different operations."
)]
pub(crate) trait Requests {
    fn save_request(&mut self) -> Result<SaveStats, &Error>;
    fn load_request(&mut self) -> Result<LoadOutcome, &Error>;
}

impl Requests for App {
    fn save_request(&mut self) -> Result<SaveStats, &Error> {
        drive(self, SaveRequest::Save)
            .last_save()
            .expect("save request did not report a result")
            .as_ref()
            .copied()
    }

    fn load_request(&mut self) -> Result<LoadOutcome, &Error> {
        drive(self, SaveRequest::Load)
            .load()
            .expect("load request did not report a result")
            .as_ref()
            .copied()
    }
}

fn drive(app: &mut App, request: SaveRequest) -> &SaveStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    let world = app.world_mut();
    world.write_message(request);
    loop {
        world.run_schedule(Last);
        let status = world.resource::<SaveStatus>();
        if !status.is_saving() && !status.is_loading() {
            break;
        }
        assert!(Instant::now() < deadline, "persistence did not settle");
        std::thread::yield_now();
    }
    world.resource::<SaveStatus>()
}
