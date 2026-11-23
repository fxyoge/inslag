use bevy_app::{App, Last};
use bevy_ecs::message::Messages;
use inslag_persist::{
    Error, SaveCompleted, SaveHandle, SaveInfo, SaveRequest, SaveStats, SaveStatus,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[allow(
    dead_code,
    reason = "Shared by test targets that exercise different operations."
)]
pub(crate) trait Requests {
    fn request(&mut self, request: SaveRequest) -> SaveCompleted;
    fn save_request(&mut self) -> Result<SaveStats, Arc<Error>>;
    fn load_request(&mut self, handle: SaveHandle) -> Result<(), Arc<Error>>;
    fn new_request(&mut self) -> SaveHandle;
    fn close_request(&mut self);
    fn list_request(&mut self) -> Result<Vec<SaveInfo>, Arc<Error>>;
    fn reload(&mut self) -> Result<(), Arc<Error>>;
}

impl Requests for App {
    fn request(&mut self, request: SaveRequest) -> SaveCompleted {
        let deadline = Instant::now() + Duration::from_secs(10);
        let world = self.world_mut();
        let mut cursor = world
            .resource::<Messages<SaveCompleted>>()
            .get_cursor_current();
        world.write_message(request);
        loop {
            world.run_schedule(Last);
            if let Some(completion) = cursor.read(world.resource()).next() {
                return completion.clone();
            }
            assert!(Instant::now() < deadline, "persistence did not settle");
            std::thread::yield_now();
        }
    }

    fn save_request(&mut self) -> Result<SaveStats, Arc<Error>> {
        match self.request(SaveRequest::Save) {
            SaveCompleted::Save {
                automatic: false,
                result,
            } => result,
            other => panic!("unexpected completion: {other:?}"),
        }
    }

    fn load_request(&mut self, handle: SaveHandle) -> Result<(), Arc<Error>> {
        match self.request(SaveRequest::Load(handle)) {
            SaveCompleted::Load {
                handle: loaded,
                result,
            } if loaded == handle => result,
            other => panic!("unexpected completion: {other:?}"),
        }
    }

    fn new_request(&mut self) -> SaveHandle {
        match self.request(SaveRequest::New) {
            SaveCompleted::New(result) => result.unwrap(),
            other => panic!("unexpected completion: {other:?}"),
        }
    }

    fn close_request(&mut self) {
        match self.request(SaveRequest::Close) {
            SaveCompleted::Close(result) => result.unwrap(),
            other => panic!("unexpected completion: {other:?}"),
        }
    }

    fn list_request(&mut self) -> Result<Vec<SaveInfo>, Arc<Error>> {
        match self.request(SaveRequest::List) {
            SaveCompleted::List(result) => result,
            other => panic!("unexpected completion: {other:?}"),
        }
    }

    fn reload(&mut self) -> Result<(), Arc<Error>> {
        let handle = self.world().resource::<SaveStatus>().handle().unwrap();
        self.close_request();
        self.load_request(handle)
    }
}

#[allow(dead_code, reason = "Not all test targets inspect SQLite directly.")]
pub(crate) fn only_file(directory: &Path) -> PathBuf {
    let mut paths = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sqlite"));
    let path = paths.next().expect("missing SQLite file");
    assert!(paths.next().is_none(), "multiple SQLite files");
    path
}
