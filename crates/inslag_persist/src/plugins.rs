use bevy_app::{Last, PreStartup};
use bevy_time::{Real, Time};

use super::*;
use crate::{
    backend::Storage,
    lifecycle::{CheckReadiness, SaveSession, has_work, process, update_interval},
};

pub struct SavePlugin {
    storage: Box<dyn Fn() -> Storage + Send + Sync>,
    interval: Option<Duration>,
    readiness: Option<ConfigureReadiness>,
}

impl SavePlugin {
    #[cfg_attr(
        not(feature = "sqlite"),
        expect(dead_code, reason = "no backend enabled")
    )]
    pub(crate) fn new(storage: impl Fn() -> Storage + Send + Sync + 'static) -> Self {
        Self {
            storage: Box::new(storage),
            interval: None,
            readiness: None,
        }
    }

    /// Save at a real-time interval, even while the simulation is paused.
    ///
    /// Install [`TimePlugin`](bevy_time::TimePlugin) first. The interval must be nonzero.
    pub fn autosave(mut self, interval: Duration) -> Self {
        assert!(!interval.is_zero(), "autosave interval must be positive");
        self.interval = Some(interval);
        self
    }

    /// Allow loading and saving only while this condition is true.
    ///
    /// Use this to wait for assets or other setup.
    pub fn enabled_when<M>(
        mut self,
        condition: impl SystemCondition<M> + Clone + Send + Sync + 'static,
    ) -> Self {
        self.readiness = Some(Box::new(move |app| {
            app.add_systems(
                CheckReadiness,
                condition.clone().pipe(lifecycle::set_enabled),
            );
        }));
        self
    }
}

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<identity::Identities>()
            .init_resource::<registration::Registry>()
            .init_resource::<SaveStatus>()
            .insert_resource((self.storage)());
        assert!(
            self.interval.is_none() || app.world().contains_resource::<Time<Real>>(),
            "autosave requires TimePlugin before SavePlugin"
        );
        app.init_schedule(PrepareSave)
            .init_schedule(SaveOpened)
            .init_schedule(SaveClosed)
            .init_schedule(CheckReadiness)
            .configure_sets(SaveOpened, (SaveSet::Rebuild, SaveSet::Ready).chain())
            .configure_sets(SaveClosed, (SaveSet::Rebuild, SaveSet::Ready).chain())
            .add_message::<SaveRequest>()
            .add_message::<SaveCompleted>()
            .init_resource::<SaveSettings>()
            .insert_resource(SaveSession::default())
            .add_systems(PreStartup, registration::initialize)
            .add_systems(
                Last,
                (
                    update_interval,
                    process.run_if(has_work.or_eager(on_message::<SaveRequest>)),
                )
                    .chain()
                    .in_set(SaveSet::Save),
            );
        if self.interval.is_some() {
            app.insert_resource(SaveSettings {
                interval: self.interval,
            });
        }
        app.persist(component::<bevy_ecs::name::Name>("bevy.name").attached())
            .persist(
                component::<bevy_transform::components::Transform>("bevy.transform").attached(),
            );
        let mut declarations: Vec<_> = inventory::iter::<registration::Declaration>
            .into_iter()
            .collect();
        declarations.sort_by_key(|declaration| declaration.key);
        for declaration in declarations {
            (declaration.install)(app);
        }
        if let Some(readiness) = &self.readiness {
            readiness(app);
        }
    }
}
