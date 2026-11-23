use bevy_app::App;
use inslag_persist::SaveStatus;
use std::time::{Duration, Instant};

pub(crate) fn settle(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = app.world().resource::<SaveStatus>();
        if !status.is_loading() && !status.is_saving() {
            return;
        }
        assert!(Instant::now() < deadline, "persistence did not settle");
        std::thread::yield_now();
        app.update();
    }
}
