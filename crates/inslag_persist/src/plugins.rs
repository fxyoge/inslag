use super::*;

#[derive(Default)]
pub struct SavePlugin {
    path: Option<PathBuf>,
    load_on_start: bool,
    interval: Option<Duration>,
    readiness: Option<ConfigureReadiness>,
}

impl SavePlugin {
    /// Open a SQLite save at startup, or start fresh if the file does not exist.
    pub fn sqlite(path: impl AsRef<Path>) -> Self {
        Self {
            path: Some(path.as_ref().to_owned()),
            load_on_start: true,
            interval: None,
            readiness: None,
        }
    }

    /// Load the configured file at startup. Enabled by default.
    ///
    /// Set this to `false` to load it later with [`SaveRequest::Load`].
    pub fn load_on_start(mut self, enabled: bool) -> Self {
        self.load_on_start = enabled;
        self
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
    /// Use this to wait for assets or other setup. A failed load still blocks
    /// saving, and direct [`WorldSaveExt::save`] calls bypass the condition.
    pub fn enabled_when<M>(
        mut self,
        condition: impl SystemCondition<M> + Clone + Send + Sync + 'static,
    ) -> Self {
        self.readiness = Some(Box::new(move |app| {
            app.add_systems(
                lifecycle::CheckReadiness,
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
            .insert_resource(SaveStatus::new(self.path.clone()))
            .insert_resource(storage::Storage::new(self.path.clone().unwrap_or_default()));
        if self.path.is_none() {
            app.world_mut().resource_mut::<storage::Storage>().close();
        }
        lifecycle::install(app, self.load_on_start, self.interval);
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
