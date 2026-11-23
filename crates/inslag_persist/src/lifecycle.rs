use crate::{
    Error, LoadOutcome, SaveStats,
    registration::Registry,
    storage::{self, Storage},
};
use bevy_ecs::{
    message::MessageCursor,
    prelude::*,
    schedule::{ScheduleLabel, SystemSet},
};
use bevy_time::{Real, Time};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SaveSet {
    Load,
    Save,
    /// Rebuild derived data after saved resources and entities are available.
    Rebuild,
    /// Run after rebuild systems and their pending commands have finished.
    Ready,
}

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PrepareSave;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SaveOpened;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SaveClosed;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CheckReadiness;

#[derive(Resource, Default, Clone, PartialEq, Eq)]
pub struct SaveSettings {
    /// Set to `None` to disable autosave.
    pub interval: Option<Duration>,
}

#[derive(Message, Clone, Debug)]
pub enum SaveRequest {
    Save,
    Load,
    Open(PathBuf),
    Close,
}

#[derive(Resource, Default)]
pub struct SaveStatus {
    path: Option<PathBuf>,
    load: Option<Result<LoadOutcome, String>>,
    last_save: Option<Result<SaveStats, String>>,
    loading: bool,
    writable: bool,
}
impl SaveStatus {
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            ..Default::default()
        }
    }
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
    pub fn load(&self) -> Option<&Result<LoadOutcome, String>> {
        self.load.as_ref()
    }
    pub fn last_save(&self) -> Option<&Result<SaveStats, String>> {
        self.last_save.as_ref()
    }
    pub fn is_loading(&self) -> bool {
        self.loading
    }
    pub fn can_save(&self) -> bool {
        self.writable
    }
}

struct Opening {
    path: PathBuf,
    storage: Storage,
}
#[derive(Resource)]
pub(crate) struct SaveSession {
    opening: Option<Opening>,
    prepared: Option<SaveRequest>,
    running: bool,
    deadline: Duration,
    interval: Option<Duration>,
    enabled: bool,
}

impl Default for SaveSession {
    fn default() -> Self {
        Self {
            opening: None,
            prepared: None,
            running: false,
            deadline: Duration::MAX,
            interval: None,
            enabled: true,
        }
    }
}

impl SaveSession {
    fn is_busy(&self) -> bool {
        self.running || self.opening.is_some() || self.prepared.is_some()
    }
}

pub(crate) fn initial_load(world: &mut World) {
    if !world.resource::<Registry>().started
        && let Some(path) = world.resource::<SaveStatus>().path.clone()
    {
        run_active(world, |world| open(world, path));
    }
}

pub(crate) fn run_idle<T>(
    world: &mut World,
    operation: impl FnOnce(&mut World) -> Result<T, Error>,
) -> Result<T, Error> {
    let session = world
        .get_resource::<SaveSession>()
        .ok_or(Error::MissingPlugin)?;
    if session.is_busy() {
        return Err(Error::Busy);
    }
    run_active(world, operation)
}

pub(crate) fn set_enabled(In(enabled): In<bool>, mut session: ResMut<SaveSession>) {
    session.enabled = enabled;
}
pub(crate) fn update_interval(
    mut session: ResMut<SaveSession>,
    settings: Res<SaveSettings>,
    time: Option<Res<Time<Real>>>,
) {
    let now = time.as_ref().map_or(Duration::ZERO, |time| time.elapsed());
    assert!(
        settings.interval.is_none() || time.is_some(),
        "autosave requires TimePlugin"
    );
    if session.interval != settings.interval {
        assert!(
            !settings.interval.is_some_and(|interval| interval.is_zero()),
            "autosave interval must be positive"
        );
        session.interval = settings.interval;
        session.deadline = settings
            .interval
            .map_or(Duration::MAX, |interval| now.saturating_add(interval));
    }
}
pub(crate) fn has_work(
    session: Res<SaveSession>,
    status: Res<SaveStatus>,
    time: Option<Res<Time<Real>>>,
) -> bool {
    let now = time.as_ref().map_or(Duration::ZERO, |time| time.elapsed());
    session.is_busy() || (status.writable && now >= session.deadline)
}
pub(crate) fn process(world: &mut World, mut cursor: Local<MessageCursor<SaveRequest>>) {
    let messages = world.resource::<Messages<SaveRequest>>();
    let mut requests = cursor.read(messages);
    let request = if world.resource::<SaveSession>().is_busy() {
        None
    } else {
        requests.next().cloned()
    };
    let rejected_save = requests.any(|request| matches!(request, SaveRequest::Save));
    cursor.clear(messages);
    if rejected_save {
        record_save(world, &Err(Error::Busy));
    }
    run_active(world, |world| process_active(world, request));
}

fn process_active(world: &mut World, request: Option<SaveRequest>) {
    if world.resource::<SaveSession>().opening.is_some() {
        finish_open(world);
        return;
    }
    let prepared = world.resource_mut::<SaveSession>().prepared.take();
    let already_prepared = prepared.is_some();
    if let Some(request) = prepared.or(request) {
        // A preparation schedule is optional. Ordinary games save immediately;
        // gameplay settlement commands get a normal cleanup frame when installed.
        let has_preparation = world
            .resource::<Schedules>()
            .get(PrepareSave)
            .is_some_and(|schedule| schedule.systems_len() > 0);
        if !already_prepared
            && !matches!(request, SaveRequest::Load)
            && world.resource::<SaveStatus>().writable
            && has_preparation
        {
            world.run_schedule(PrepareSave);
            world.flush();
            world.resource_mut::<SaveSession>().prepared = Some(request);
            return;
        }
        match request {
            SaveRequest::Save => {
                let _ = save(world);
            }
            SaveRequest::Load => {
                if let Some(path) = world.resource::<SaveStatus>().path.clone() {
                    open(world, path);
                }
            }
            SaveRequest::Open(path) => {
                if !outgoing(world) || save(world).is_ok() {
                    open(world, path);
                }
            }
            SaveRequest::Close => {
                if outgoing(world) && save(world).is_ok() {
                    storage::clear_scope(world);
                    world.resource_mut::<Storage>().close();
                    *world.resource_mut::<SaveStatus>() = SaveStatus::default();
                    world.run_schedule(SaveClosed);
                }
            }
        }
        return;
    }
    let now = world
        .get_resource::<Time<Real>>()
        .map_or(Duration::ZERO, |time| time.elapsed());
    if world.resource::<SaveStatus>().writable && now >= world.resource::<SaveSession>().deadline {
        let _ = save(world);
        reset_deadline(world);
    }
}
fn save(world: &mut World) -> Result<(), Error> {
    let result = if ready(world) {
        storage::save(world)
    } else {
        Err(Error::NotReady)
    };
    record_save(world, &result);
    result.map(|_| ())
}
pub(crate) fn record_save(world: &mut World, result: &Result<SaveStats, Error>) {
    let mut status = world.resource_mut::<SaveStatus>();
    status.last_save = Some(result.as_ref().copied().map_err(ToString::to_string));
    if result.is_ok() {
        status.writable = true;
        reset_deadline(world);
    }
}
fn run_active<T>(world: &mut World, operation: impl FnOnce(&mut World) -> T) -> T {
    world.resource_mut::<SaveSession>().running = true;
    let result = operation(world);
    world.resource_mut::<SaveSession>().running = false;
    result
}
fn open(world: &mut World, path: PathBuf) {
    world.resource_mut::<SaveSession>().opening = Some(Opening {
        storage: Storage::sqlite(path.clone()),
        path,
    });
    let mut status = world.resource_mut::<SaveStatus>();
    status.loading = true;
    status.load = None;
    finish_open(world);
}
fn finish_open(world: &mut World) {
    if !ready(world) {
        return;
    }
    let Some(opening) = world.resource_mut::<SaveSession>().opening.take() else {
        return;
    };
    let outgoing = world
        .remove_resource::<Storage>()
        .expect("installed save storage");
    world.insert_resource(opening.storage);
    let result = storage::load(world);
    match result {
        Ok(outcome) => {
            if outcome == LoadOutcome::Missing {
                storage::clear_scope(world);
            }
            *world.resource_mut::<SaveStatus>() = SaveStatus {
                path: Some(opening.path),
                load: Some(Ok(outcome)),
                writable: true,
                ..Default::default()
            };
            world.run_schedule(SaveOpened);
            reset_deadline(world);
        }
        Err(Error::AssetNotReady(_)) => {
            let storage = world.remove_resource::<Storage>().unwrap();
            world.insert_resource(outgoing);
            world.resource_mut::<SaveSession>().opening = Some(Opening {
                path: opening.path,
                storage,
            });
        }
        Err(error) => {
            world.insert_resource(outgoing);
            let writable = storage::can_save(world);
            let mut status = world.resource_mut::<SaveStatus>();
            status.loading = false;
            status.load = Some(Err(error.to_string()));
            status.writable = writable;
        }
    }
}
fn outgoing(world: &World) -> bool {
    !world.resource::<Storage>().is_closed()
        && (world.resource::<SaveStatus>().writable
            || (world.resource::<Registry>().started
                && world.resource::<SaveStatus>().load.is_none()))
}
fn ready(world: &mut World) -> bool {
    world.run_schedule(CheckReadiness);
    world.resource::<SaveSession>().enabled
}
pub(crate) fn record_load(world: &mut World, result: &Result<LoadOutcome, Error>) {
    let writable = storage::can_save(world);
    let mut status = world.resource_mut::<SaveStatus>();
    status.load = Some(result.as_ref().copied().map_err(ToString::to_string));
    status.writable = writable;
    if result.is_ok() {
        world.run_schedule(SaveOpened);
        reset_deadline(world);
    }
}
fn reset_deadline(world: &mut World) {
    let now = world
        .get_resource::<Time<Real>>()
        .map_or(Duration::ZERO, |time| time.elapsed());
    let mut session = world.resource_mut::<SaveSession>();
    session.deadline = session
        .interval
        .map_or(Duration::MAX, |interval| now.saturating_add(interval));
}
