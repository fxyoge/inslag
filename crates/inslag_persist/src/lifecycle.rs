use crate::{
    Error, SaveHandle, SaveInfo, SaveStats,
    backend::{Pending, Storage, StoredSave},
    storage::{self, Saving},
};
use bevy_ecs::{
    message::MessageCursor,
    prelude::*,
    schedule::{ScheduleLabel, SystemSet},
};
use bevy_time::{Real, Time};
use std::{sync::Arc, time::Duration};

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SaveSet {
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

#[derive(Message, Clone, Copy, Debug)]
pub enum SaveRequest {
    List,
    New,
    Load(SaveHandle),
    Save,
    Close,
}

#[derive(Message, Clone, Debug)]
pub enum SaveCompleted {
    List(Result<Vec<SaveInfo>, Arc<Error>>),
    New(Result<SaveHandle, Arc<Error>>),
    Load {
        handle: SaveHandle,
        result: Result<(), Arc<Error>>,
    },
    Save {
        automatic: bool,
        result: Result<SaveStats, Arc<Error>>,
    },
    Close(Result<(), Arc<Error>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadOutcome {
    Fresh,
    Loaded,
}

#[derive(Resource, Default)]
pub struct SaveStatus {
    handle: Option<SaveHandle>,
    load: Option<Result<LoadOutcome, Arc<Error>>>,
    last_save: Option<Result<SaveStats, Arc<Error>>>,
    loading: bool,
    saving: bool,
}

impl SaveStatus {
    pub fn handle(&self) -> Option<SaveHandle> {
        self.handle
    }

    pub fn load(&self) -> Option<&Result<LoadOutcome, Arc<Error>>> {
        self.load.as_ref()
    }

    pub fn last_save(&self) -> Option<&Result<SaveStats, Arc<Error>>> {
        self.last_save.as_ref()
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn is_saving(&self) -> bool {
        self.saving
    }

    pub fn can_save(&self) -> bool {
        self.handle.is_some() && !self.saving
    }
}

enum Read {
    Waiting,
    Pending(Pending<StoredSave>),
    Ready(StoredSave),
}

enum Operation {
    New,
    Load(SaveHandle, Read),
    Prepare(SaveHandle, bool),
    Write(Saving, bool),
}

#[derive(Resource)]
pub(crate) struct SaveSession {
    operation: Option<Operation>,
    listing: Option<Pending<Vec<SaveInfo>>>,
    deadline: Duration,
    interval: Option<Duration>,
    enabled: bool,
}

impl Default for SaveSession {
    fn default() -> Self {
        Self {
            operation: None,
            listing: None,
            deadline: Duration::MAX,
            interval: None,
            enabled: true,
        }
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
        session.deadline = settings.interval.map_or(Duration::MAX, |interval| {
            time.as_ref().unwrap().elapsed().saturating_add(interval)
        });
    }
}

pub(crate) fn has_work(
    session: Res<SaveSession>,
    status: Res<SaveStatus>,
    time: Option<Res<Time<Real>>>,
) -> bool {
    session.operation.is_some()
        || session.listing.is_some()
        || (status.can_save() && elapsed(time.as_deref()) >= session.deadline)
}

pub(crate) fn process(world: &mut World, mut cursor: Local<MessageCursor<SaveRequest>>) {
    let requests: Vec<_> = cursor.read(world.resource()).copied().collect();
    let mut session_claimed = world.resource::<SaveSession>().operation.is_some();
    let mut list_claimed = world.resource::<SaveSession>().listing.is_some();
    if session_claimed {
        advance(world);
    }
    if list_claimed {
        poll_list(world);
    }
    for request in requests {
        if matches!(request, SaveRequest::List) {
            if list_claimed {
                reject(world, request, Error::Busy);
            } else {
                start_list(world);
                list_claimed = true;
            }
        } else if session_claimed {
            reject(world, request, Error::Busy);
        } else {
            handle_request(world, request);
            session_claimed = true;
        }
    }
    if !session_claimed
        && world.resource::<SaveStatus>().can_save()
        && elapsed(world.get_resource::<Time<Real>>()) >= world.resource::<SaveSession>().deadline
        && let Some(handle) = world.resource::<SaveStatus>().handle()
    {
        start_save(world, handle, true);
    }
}

fn handle_request(world: &mut World, request: SaveRequest) {
    let active = world.resource::<SaveStatus>().handle();
    match request {
        SaveRequest::New | SaveRequest::Load(_) if active.is_some() => {
            reject(world, request, Error::AlreadyOpen);
        }
        SaveRequest::New | SaveRequest::Load(_) => {
            let operation = match request {
                SaveRequest::New => Operation::New,
                SaveRequest::Load(handle) => Operation::Load(handle, Read::Waiting),
                _ => unreachable!(),
            };
            let mut status = world.resource_mut::<SaveStatus>();
            status.loading = true;
            status.load = None;
            world.resource_mut::<SaveSession>().operation = Some(operation);
            advance(world);
        }
        SaveRequest::Save | SaveRequest::Close if active.is_none() => {
            reject(world, request, Error::Closed);
        }
        SaveRequest::Save => start_save(world, active.expect("active save"), false),
        SaveRequest::Close => {
            storage::clear_scope(world);
            *world.resource_mut::<SaveStatus>() = SaveStatus::default();
            reset_deadline(world);
            world.run_schedule(SaveClosed);
            world.write_message(SaveCompleted::Close(Ok(())));
        }
        SaveRequest::List => unreachable!(),
    }
}

fn advance(world: &mut World) {
    let operation = world.resource_mut::<SaveSession>().operation.take();
    let pending = match operation {
        Some(Operation::New) if ready(world) => {
            let result = new_session(world).map_err(Arc::new);
            finish_open(world, result.clone(), LoadOutcome::Fresh);
            world.write_message(SaveCompleted::New(result));
            None
        }
        Some(Operation::Load(handle, read)) => advance_load(world, handle, read),
        Some(Operation::Prepare(handle, automatic)) => start_write(world, handle, automatic, false),
        Some(Operation::Write(mut saving, automatic)) => match saving.pending.poll() {
            Some(result) => {
                saving.finish(world, &result);
                finish_save(world, result, automatic);
                None
            }
            None => Some(Operation::Write(saving, automatic)),
        },
        pending => pending,
    };
    world.resource_mut::<SaveSession>().operation = pending;
}

fn poll_list(world: &mut World) {
    let mut session = world.resource_mut::<SaveSession>();
    if let Some(result) = session.listing.as_mut().and_then(Pending::poll) {
        session.listing = None;
        world.write_message(SaveCompleted::List(result.map_err(Arc::new)));
    }
}

fn start_list(world: &mut World) {
    match world.resource::<Storage>().list() {
        Ok(pending) => world.resource_mut::<SaveSession>().listing = Some(pending),
        Err(error) => reject(world, SaveRequest::List, error),
    }
}

fn reject(world: &mut World, request: SaveRequest, error: Error) {
    let error = Arc::new(error);
    let completion = match request {
        SaveRequest::List => SaveCompleted::List(Err(error)),
        SaveRequest::New => SaveCompleted::New(Err(error)),
        SaveRequest::Load(handle) => SaveCompleted::Load {
            handle,
            result: Err(error),
        },
        SaveRequest::Save => SaveCompleted::Save {
            automatic: false,
            result: Err(error),
        },
        SaveRequest::Close => SaveCompleted::Close(Err(error)),
    };
    world.write_message(completion);
}

fn advance_load(world: &mut World, handle: SaveHandle, read: Read) -> Option<Operation> {
    if !ready(world) {
        return Some(Operation::Load(handle, read));
    }
    let result = match read {
        Read::Waiting => match world.resource::<Storage>().read(handle) {
            Ok(pending) => return Some(Operation::Load(handle, Read::Pending(pending))),
            Err(error) => Err(error),
        },
        Read::Pending(mut pending) => match pending.poll() {
            Some(result) => result,
            None => return Some(Operation::Load(handle, Read::Pending(pending))),
        },
        Read::Ready(saved) => Ok(saved),
    };
    let result = match result {
        Ok(saved) => match storage::restore(world, &saved) {
            Err(Error::AssetNotReady(_)) => {
                return Some(Operation::Load(handle, Read::Ready(saved)));
            }
            result => result,
        },
        Err(error) => Err(error),
    }
    .map_err(Arc::new);
    finish_open(world, result.clone().map(|()| handle), LoadOutcome::Loaded);
    world.write_message(SaveCompleted::Load { handle, result });
    None
}

fn start_save(world: &mut World, handle: SaveHandle, automatic: bool) {
    let mut status = world.resource_mut::<SaveStatus>();
    status.saving = true;
    status.last_save = None;
    let pending = start_write(world, handle, automatic, true);
    world.resource_mut::<SaveSession>().operation = pending;
}

fn start_write(
    world: &mut World,
    handle: SaveHandle,
    automatic: bool,
    allow_preparation: bool,
) -> Option<Operation> {
    if !ready(world) {
        finish_save(world, Err(Error::NotReady), automatic);
        return None;
    }
    if allow_preparation
        && world
            .resource::<Schedules>()
            .get(PrepareSave)
            .is_some_and(|schedule| schedule.systems_len() > 0)
    {
        // Give gameplay settlement commands a normal cleanup frame before capture.
        world.run_schedule(PrepareSave);
        world.flush();
        return Some(Operation::Prepare(handle, automatic));
    }
    match storage::begin_save(world, handle) {
        Ok(saving) => Some(Operation::Write(saving, automatic)),
        Err(error) => {
            finish_save(world, Err(error), automatic);
            None
        }
    }
}

fn ready(world: &mut World) -> bool {
    world.run_schedule(CheckReadiness);
    world.resource::<SaveSession>().enabled
}

fn new_session(world: &mut World) -> Result<SaveHandle, Error> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|error| Error::Io(Box::new(error)))?;
    storage::start_fresh(world);
    Ok(SaveHandle(u128::from_be_bytes(bytes)))
}

fn finish_open(world: &mut World, result: Result<SaveHandle, Arc<Error>>, outcome: LoadOutcome) {
    let mut status = world.resource_mut::<SaveStatus>();
    status.loading = false;
    status.handle = result.as_ref().ok().copied();
    status.load = Some(result.map(|_| outcome));
    if status.handle.is_some() {
        reset_deadline(world);
        world.run_schedule(SaveOpened);
    }
}

fn finish_save(world: &mut World, result: Result<SaveStats, Error>, automatic: bool) {
    let result = result.map_err(Arc::new);
    let mut status = world.resource_mut::<SaveStatus>();
    status.saving = false;
    status.last_save = Some(result.clone());
    if automatic || result.is_ok() {
        reset_deadline(world);
    }
    world.write_message(SaveCompleted::Save { automatic, result });
}

fn reset_deadline(world: &mut World) {
    let now = elapsed(world.get_resource::<Time<Real>>());
    let mut session = world.resource_mut::<SaveSession>();
    session.deadline = session
        .interval
        .map_or(Duration::MAX, |interval| now.saturating_add(interval));
}

fn elapsed(time: Option<&Time<Real>>) -> Duration {
    time.map_or(Duration::ZERO, Time::elapsed)
}
