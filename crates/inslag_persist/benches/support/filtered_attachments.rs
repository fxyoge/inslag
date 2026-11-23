use super::requests::Requests;
use bevy_app::{App, Last};
use bevy_ecs::prelude::*;
use divan::Bencher;
use inslag_persist::{AppSaveExt, Capture, SavePlugin, SaveRequest, SaveStatus, component};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Component, Serialize, Deserialize)]
struct Owner;

#[derive(Component, Serialize, Deserialize)]
struct Attachment(u32);

#[derive(Component)]
struct Selected;

#[divan::bench(consts = [1_000, 10_000, 100_000], args = [0, 10], sample_size = 1, sample_count = 16)]
fn attachment_capture<const N: usize>(bencher: Bencher, deselected_percent: usize) {
    bencher
        .with_inputs(|| Fixture::new(N, 0, deselected_percent))
        .bench_local_refs(Fixture::capture);
}

#[divan::bench(consts = [1_000, 10_000, 100_000], args = [0, 10], sample_size = 1, sample_count = 16)]
fn attachment_commit<const N: usize>(bencher: Bencher, deselected_percent: usize) {
    bencher
        .with_inputs(|| Fixture::new(N, 0, deselected_percent))
        .bench_local_refs(Fixture::commit);
}

#[divan::bench(args = [0, 10], sample_size = 1, sample_count = 16)]
fn attachment_sparse_capture(bencher: Bencher, deselected_percent: usize) {
    bencher
        .with_inputs(|| Fixture::new(10_000, 90_000, deselected_percent))
        .bench_local_refs(Fixture::capture);
}

#[divan::bench(args = [0, 10], sample_size = 1, sample_count = 16)]
fn attachment_sparse_commit(bencher: Bencher, deselected_percent: usize) {
    bencher
        .with_inputs(|| Fixture::new(10_000, 90_000, deselected_percent))
        .bench_local_refs(Fixture::commit);
}

struct Fixture {
    app: App,
    _directory: tempfile::TempDir,
    entities: usize,
    deselected: usize,
}

impl Fixture {
    fn new(entities: usize, unowned: usize, deselected_percent: usize) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new();
        app.add_plugins(SavePlugin::sqlite(directory.path()))
            .persist(component::<Owner>("owner").capture(Capture::Dirty))
            .persist(
                component::<Attachment>("attachment")
                    .attached()
                    .filter::<With<Selected>>(),
            );
        app.new_request();
        let world = app.world_mut();
        let spawned: Vec<_> = world
            .spawn_batch((0..entities).map(|i| (Owner, Attachment(i as u32), Selected)))
            .collect();
        drop(world.spawn_batch((0..unowned).map(|i| (Attachment(i as u32), Selected))));
        assert_eq!(app.save_request().unwrap().upserts, entities * 2);
        assert_eq!(app.save_request().unwrap().upserts, entities);
        let deselected = entities * deselected_percent / 100;
        for &entity in &spawned[..deselected] {
            app.world_mut().entity_mut(entity).remove::<Selected>();
        }
        Self {
            app,
            _directory: directory,
            entities,
            deselected,
        }
    }

    fn commit(&mut self) {
        self.capture();
        self.wait();
    }

    fn capture(&mut self) {
        let world = self.app.world_mut();
        world.write_message(SaveRequest::Save);
        world.run_schedule(Last);
        assert!(world.resource::<SaveStatus>().is_saving());
    }

    fn wait(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let world = self.app.world_mut();
        while world.resource::<SaveStatus>().is_saving() {
            world.run_schedule(Last);
            assert!(Instant::now() < deadline, "attachment save did not settle");
            std::thread::yield_now();
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.wait();
            let status = self.app.world().resource::<SaveStatus>();
            let stats = status.last_save().unwrap().as_ref().unwrap();
            assert_eq!(stats.generation, 3);
            assert_eq!(stats.entities, self.entities);
            assert_eq!(stats.upserts, self.entities - self.deselected);
            assert_eq!(stats.deletions, self.deselected);
        }
    }
}
