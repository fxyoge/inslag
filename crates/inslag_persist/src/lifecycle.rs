use crate::{
    Error, LoadOutcome, SaveStats,
    registration::Registry,
    storage::{self, Pending, Saving, Storage, StoredSave},
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
    load: Option<Result<LoadOutcome, Error>>,
    last_save: Option<Result<SaveStats, Error>>,
    loading: bool,
    saving: bool,
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
    pub fn load(&self) -> Option<&Result<LoadOutcome, Error>> {
        self.load.as_ref()
    }
    pub fn last_save(&self) -> Option<&Result<SaveStats, Error>> {
        self.last_save.as_ref()
    }
    pub fn is_loading(&self) -> bool {
        self.loading
    }
    pub fn is_saving(&self) -> bool {
        self.saving
    }
    pub fn can_save(&self) -> bool {
        self.writable
    }
}

struct Opening {
    path: PathBuf,
    storage: Storage,
    read: Read,
}

enum Read {
    Waiting,
    Pending(Pending<Option<StoredSave>>),
    Ready(Option<StoredSave>),
}

#[derive(Default)]
enum AfterSave {
    #[default]
    None,
    Open(PathBuf),
    Close,
}

#[derive(Resource)]
pub(crate) struct SaveSession {
    opening: Option<Opening>,
    saving: Option<(Saving, AfterSave)>,
    prepared: Option<SaveRequest>,
    queued: Option<SaveRequest>,
    deadline: Duration,
    interval: Option<Duration>,
    enabled: bool,
}

impl Default for SaveSession {
    fn default() -> Self {
        Self {
            opening: None,
            saving: None,
            prepared: None,
            queued: None,
            deadline: Duration::MAX,
            interval: None,
            enabled: true,
        }
    }
}

impl SaveSession {
    fn is_busy(&self) -> bool {
        self.opening.is_some()
            || self.saving.is_some()
            || self.prepared.is_some()
            || self.queued.is_some()
    }
}

pub(crate) fn initial_load(world: &mut World) {
    if !world.resource::<Registry>().started
        && let Some(path) = world.resource::<SaveStatus>().path.clone()
    {
        open(world, path);
    }
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
    let session = world.resource::<SaveSession>();
    let accepts_save = matches!(session.saving, Some((_, AfterSave::None)))
        || matches!(session.queued, Some(SaveRequest::Save));
    let accepts_follow_up = accepts_save || matches!(session.prepared, Some(SaveRequest::Save));
    // A transition must not get swallowed by an autosave or a redundant save request.
    let queued = if accepts_follow_up && matches!(session.queued, None | Some(SaveRequest::Save)) {
        let first = requests.next();
        first
            .filter(|request| !matches!(request, SaveRequest::Save))
            .or_else(|| requests.find(|request| !matches!(request, SaveRequest::Save)))
            .or(first.filter(|_| accepts_save))
            .cloned()
    } else {
        None
    };
    let request = if session.is_busy() {
        None
    } else {
        requests.next().cloned()
    };
    let rejected_save = !accepts_follow_up
        && session.saving.is_none()
        && requests.any(|request| matches!(request, SaveRequest::Save));
    cursor.clear(messages);
    if let Some(queued) = queued {
        world.resource_mut::<SaveSession>().queued = Some(queued);
    }
    if rejected_save {
        record_save(world, Err(Error::Busy));
    }
    process_active(world, request);
}

fn process_active(world: &mut World, request: Option<SaveRequest>) {
    if world.resource::<SaveSession>().saving.is_some() {
        finish_write(world);
        return;
    }
    if world.resource::<SaveSession>().opening.is_some() {
        finish_open(world);
        return;
    }
    let prepared = world.resource_mut::<SaveSession>().prepared.take();
    let already_prepared = prepared.is_some();
    if already_prepared {
        world.resource_mut::<SaveStatus>().saving = false;
    }
    let request = prepared
        .or_else(|| world.resource_mut::<SaveSession>().queued.take())
        .or(request);
    if let Some(request) = request {
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
            let mut status = world.resource_mut::<SaveStatus>();
            status.saving = true;
            status.last_save = None;
            return;
        }
        match request {
            SaveRequest::Save => start_write(world, AfterSave::None),
            SaveRequest::Load => {
                if let Some(path) = world.resource::<SaveStatus>().path.clone() {
                    open(world, path);
                } else {
                    world.resource_mut::<SaveStatus>().load = Some(Err(Error::Closed));
                }
            }
            SaveRequest::Open(path) => {
                if outgoing(world) {
                    start_write(world, AfterSave::Open(path));
                } else {
                    open(world, path);
                }
            }
            SaveRequest::Close => {
                if outgoing(world) {
                    start_write(world, AfterSave::Close);
                }
            }
        }
        return;
    }
    let now = world
        .get_resource::<Time<Real>>()
        .map_or(Duration::ZERO, |time| time.elapsed());
    if world.resource::<SaveStatus>().writable && now >= world.resource::<SaveSession>().deadline {
        start_write(world, AfterSave::None);
        reset_deadline(world);
    }
}
fn finish_write(world: &mut World) {
    let mut session = world.resource_mut::<SaveSession>();
    let (saving, _) = session.saving.as_mut().expect("pending write");
    let Some(result) = saving.pending.poll() else {
        return;
    };
    let (saving, after) = session.saving.take().expect("pending write");
    if result.is_err() {
        session.queued = None;
    }
    saving.finish(world, &result);
    world.resource_mut::<SaveStatus>().saving = false;
    let committed = result.is_ok();
    record_save(world, result);
    if committed {
        match after {
            AfterSave::None => {}
            AfterSave::Open(path) => open(world, path),
            AfterSave::Close => {
                storage::clear_scope(world);
                world.resource_mut::<Storage>().close();
                *world.resource_mut::<SaveStatus>() = SaveStatus::default();
                world.run_schedule(SaveClosed);
            }
        }
    }
}

fn open(world: &mut World, path: PathBuf) {
    world.resource_mut::<SaveSession>().opening = Some(Opening {
        storage: Storage::sqlite(path.clone()),
        path,
        read: Read::Waiting,
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
    let Some(mut opening) = world.resource_mut::<SaveSession>().opening.take() else {
        return;
    };
    let read_error = match &mut opening.read {
        Read::Waiting => {
            world.resource_mut::<Registry>().started = true;
            match opening.storage.read() {
                Ok(pending) => {
                    opening.read = Read::Pending(pending);
                    world.resource_mut::<SaveSession>().opening = Some(opening);
                    return;
                }
                Err(error) => Some(error),
            }
        }
        Read::Pending(pending) => match pending.poll() {
            None => {
                world.resource_mut::<SaveSession>().opening = Some(opening);
                return;
            }
            Some(Ok(saved)) => {
                opening.read = Read::Ready(saved);
                None
            }
            Some(Err(error)) => Some(error),
        },
        Read::Ready(_) => None,
    };
    let result = if let Some(error) = read_error {
        Err(error)
    } else if let Read::Ready(saved) = &opening.read {
        storage::restore(world, saved.as_ref())
    } else {
        unreachable!("read has either completed or failed")
    };
    match result {
        Ok(outcome) => {
            world.insert_resource(opening.storage);
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
            world.resource_mut::<SaveSession>().opening = Some(opening);
        }
        Err(error) => {
            if world.resource::<SaveStatus>().path() == Some(&opening.path) {
                world.resource_mut::<Storage>().block_writes();
            }
            let writable = storage::can_save(world);
            let mut status = world.resource_mut::<SaveStatus>();
            status.loading = false;
            status.load = Some(Err(error));
            status.writable = writable;
        }
    }
}
fn start_write(world: &mut World, after: AfterSave) {
    let result = if ready(world) {
        storage::begin_save(world)
    } else {
        Err(Error::NotReady)
    };
    match result {
        Ok(saving) => {
            world.resource_mut::<SaveSession>().saving = Some((saving, after));
            let mut status = world.resource_mut::<SaveStatus>();
            status.saving = true;
            status.last_save = None;
        }
        Err(error) => {
            world.resource_mut::<SaveSession>().queued = None;
            record_save(world, Err(error));
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
fn record_save(world: &mut World, result: Result<SaveStats, Error>) {
    let mut status = world.resource_mut::<SaveStatus>();
    let committed = result.is_ok();
    status.last_save = Some(result);
    if committed {
        status.writable = true;
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
