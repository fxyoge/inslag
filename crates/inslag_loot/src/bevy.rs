use crate::{
    Arguments, Catalog, FunctionCall, FunctionSignature, FunctionSignatures, Limits, LootArgs,
    LootError, LootOutput, LootReturn, LootValue, Session, SourceDocument, Step, Value,
};
use bevy_app::{App, Plugin, PostUpdate, PreUpdate};
use bevy_asset::{
    Asset, AssetApp, AssetEvent, AssetLoader, AssetPath, AssetServer, Assets, AsyncReadExt, Handle,
    LoadContext, LoadState, io::Reader,
};
use bevy_ecs::{
    prelude::*,
    schedule::common_conditions::on_message,
    system::{ReadOnlySystemParam, SystemParamItem, SystemParamValidationError, SystemState},
};
use bevy_reflect::TypePath;
use std::{
    any::TypeId,
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

/// Randomness available to a loot generator during one call.
pub struct LootFunctionContext {
    random: fastrand::Rng,
}

impl LootFunctionContext {
    /// Returns the generator call's RNG, seeded from [`LootRequest::seed`].
    pub fn rng(&mut self) -> &mut fastrand::Rng {
        &mut self.random
    }
    fn new(call: &FunctionCall) -> Self {
        Self {
            random: fastrand::Rng::with_seed(call.seed),
        }
    }
}

type PureGenerator =
    dyn Fn(&mut LootFunctionContext, &mut Arguments) -> Result<Vec<Value>, String> + Send + Sync;

mod function;
pub use function::LootFunction;

fn typed_signature<A: LootArgs, R: LootReturn>() -> FunctionSignature {
    let mut signature = A::signature();
    signature.output = R::output_type();
    signature
}

#[derive(Clone)]
enum Generator {
    Pure(Arc<PureGenerator>),
    Contextual,
}

#[derive(Default)]
struct RegistryData {
    signatures: FunctionSignatures,
    generators: HashMap<String, Generator>,
}

#[derive(Resource, Default)]
pub(crate) struct LootRegistry(RegistryData);

impl LootRegistry {
    #[cfg(test)]
    pub fn signatures(&self) -> FunctionSignatures {
        self.0.signatures.clone()
    }

    pub fn compile(
        &self,
        sources: impl IntoIterator<Item = SourceDocument>,
    ) -> Result<Arc<Catalog>, LootError> {
        Catalog::compile(sources, &self.0.signatures).map(Arc::new)
    }

    #[cfg(test)]
    pub fn generate<T: LootOutput>(
        &self,
        table: &crate::LootTable<T>,
        inputs: BTreeMap<String, Value>,
        seed: u64,
    ) -> Result<Vec<T>, LootError> {
        crate::typed::convert_outputs(self.generate_values(
            table.catalog.clone(),
            &table.name,
            inputs,
            seed,
        )?)
    }

    #[cfg(test)]
    fn generate_values(
        &self,
        catalog: Arc<Catalog>,
        table: &str,
        inputs: BTreeMap<String, Value>,
        seed: u64,
    ) -> Result<Vec<Value>, LootError> {
        let mut session = Session::new(catalog, table, inputs, seed)?;
        loop {
            match session.advance()? {
                Step::Finished(generated) => return Ok(generated),
                Step::Call(call) => {
                    let generator = self.generator(&call)?;
                    let Generator::Pure(generator) = generator else {
                        return Err(LootError::new(
                            call.location,
                            "generator needs read-only Bevy context; submit a LootRequest",
                        ));
                    };
                    session.resume(invoke(&generator, call))?;
                }
                Step::Waiting => unreachable!("synchronous generators are completed immediately"),
            }
        }
    }

    fn generator(&self, call: &FunctionCall) -> Result<Generator, LootError> {
        let registry = &self.0;
        if registry.signatures.get(&call.function) != Some(&call.signature) {
            return Err(LootError::new(
                call.location.clone(),
                format!(
                    "generator {:?} is not registered with the catalog's signature",
                    call.function
                ),
            ));
        }
        registry
            .generators
            .get(&call.function)
            .cloned()
            .ok_or_else(|| LootError::new(call.location.clone(), "generator is not registered"))
    }

    fn register(&mut self, name: &str, signature: FunctionSignature, generator: Generator) {
        let registry = &mut self.0;
        registry
            .signatures
            .register(name, signature)
            .unwrap_or_else(|error| panic!("{error}"));
        registry.generators.insert(name.into(), generator);
    }
}

fn invoke(generator: &Arc<PureGenerator>, mut call: FunctionCall) -> Result<Vec<Value>, String> {
    let mut context = LootFunctionContext::new(&call);
    let generated = generator(&mut context, &mut call.arguments)?;
    call.arguments.finish()?;
    Ok(generated)
}

/// Loot registration for Bevy apps.
pub trait LootAppExt {
    /// Registers a loot function under the name used to call it in `.loot` files.
    ///
    /// # Panics
    ///
    /// Panics if the name is already registered or the name or argument declaration
    /// is invalid. Loot functions using Bevy system parameters also panic if
    /// [`LootPlugin`] is not installed or their parameters allow deferred mutations.
    fn add_loot_function<M>(&mut self, name: &str, generator: impl LootFunction<M>) -> &mut Self;

    /// Enables requests for this output type. Repeated registration is harmless.
    ///
    /// # Panics
    ///
    /// Panics if [`LootPlugin`] has not been installed.
    fn add_loot_output<T: LootOutput>(&mut self) -> &mut Self;
}

impl LootAppExt for App {
    fn add_loot_function<M>(&mut self, name: &str, generator: impl LootFunction<M>) -> &mut Self {
        function::register(self, name, generator);
        self
    }

    fn add_loot_output<T: LootOutput>(&mut self) -> &mut Self {
        assert!(
            self.world().contains_resource::<Runtime>(),
            "install LootPlugin before registering outputs"
        );
        self.init_resource::<OutputRegistrations>();
        if !self
            .world_mut()
            .resource_mut::<OutputRegistrations>()
            .0
            .insert(TypeId::of::<T>())
        {
            return self;
        }
        self.add_message::<LootRequest<T>>()
            .add_message::<LootFinished<T>>()
            .add_systems(
                PostUpdate,
                relay_requests::<T>.in_set(LootSet::Evaluate).before(drive),
            )
            .add_systems(PostUpdate, relay_finished::<T>.in_set(LootSet::Publish));
        self
    }
}

trait GeneratorAppExt {
    fn register_generator<F>(
        &mut self,
        name: &str,
        signature: FunctionSignature,
        generator: F,
    ) -> &mut Self
    where
        F: Fn(&mut LootFunctionContext, &mut Arguments) -> Result<Vec<Value>, String>
            + Send
            + Sync
            + 'static;

    fn register_contextual<P, F>(
        &mut self,
        name: &str,
        signature: FunctionSignature,
        generator: F,
    ) -> &mut Self
    where
        P: ReadOnlySystemParam + 'static,
        F: for<'w, 's> Fn(
                &mut LootFunctionContext,
                &mut Arguments,
                SystemParamItem<'w, 's, P>,
            ) -> Result<Vec<Value>, String>
            + Send
            + Sync
            + 'static;
}

impl GeneratorAppExt for App {
    fn register_generator<F>(
        &mut self,
        name: &str,
        signature: FunctionSignature,
        generator: F,
    ) -> &mut Self
    where
        F: Fn(&mut LootFunctionContext, &mut Arguments) -> Result<Vec<Value>, String>
            + Send
            + Sync
            + 'static,
    {
        self.init_resource::<LootRegistry>();
        self.world_mut().resource_mut::<LootRegistry>().register(
            name,
            signature,
            Generator::Pure(Arc::new(generator)),
        );
        self
    }

    fn register_contextual<P, F>(
        &mut self,
        name: &str,
        signature: FunctionSignature,
        generator: F,
    ) -> &mut Self
    where
        P: ReadOnlySystemParam + 'static,
        F: for<'w, 's> Fn(
                &mut LootFunctionContext,
                &mut Arguments,
                SystemParamItem<'w, 's, P>,
            ) -> Result<Vec<Value>, String>
            + Send
            + Sync
            + 'static,
    {
        assert!(
            self.world().contains_resource::<Runtime>(),
            "install LootPlugin before contextual generators"
        );
        // Commands are read-only SystemParams in Bevy, but would turn a generator into a command sink.
        assert!(
            !SystemState::<P>::new(self.world_mut())
                .meta()
                .has_deferred(),
            "loot generators cannot use deferred world mutations"
        );
        self.world_mut().resource_mut::<LootRegistry>().register(
            name,
            signature,
            Generator::Contextual,
        );
        let name = name.to_string();
        self.add_systems(
            PostUpdate,
            (move |mut calls: MessageReader<NativeCall>,
                   results: bevy_ecs::system::ParallelCommands,
                   runtime: Res<Runtime>,
                   mut params: ParamSet<(Result<P, SystemParamValidationError>,)>| {
                for request in calls.read() {
                    if request.call.function != name || !runtime.current(request.token) {
                        continue;
                    }
                    let mut arguments = request.call.arguments.clone();
                    let mut context = LootFunctionContext::new(&request.call);
                    // Each call needs fresh parameter borrows without cloning ECS data.
                    let generated = params
                        .p0()
                        .map_err(|error| {
                            format!(
                                "invalid context for {name:?} ({}): {error}",
                                std::any::type_name::<P>()
                            )
                        })
                        .and_then(|params| {
                            generator(&mut context, &mut arguments, params).and_then(|generated| {
                                arguments.finish()?;
                                Ok(generated)
                            })
                        });
                    let token = request.token;
                    results.command_scope(move |mut commands| {
                        commands.write_message(NativeResult { token, generated });
                    });
                }
            })
            .in_set(LootSet::Generate)
            .run_if(on_message::<NativeCall>),
        );
        self
    }
}

#[cfg(test)]
mod tests;

#[derive(Asset, TypePath)]
struct LootSource {
    text: String,
}

#[derive(Default, TypePath)]
struct LootLoader;

impl AssetLoader for LootLoader {
    type Asset = LootSource;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<LootSource, std::io::Error> {
        let mut text = String::new();
        reader.read_to_string(&mut text).await?;
        Ok(LootSource { text })
    }
    fn extensions(&self) -> &[&str] {
        &["loot"]
    }
}

enum PreparedLoot {
    Loading,
    Ready(Arc<Catalog>),
    Failed(LootError),
}

#[derive(Resource)]
struct Sources {
    handles: Vec<Handle<LootSource>>,
    prepared: PreparedLoot,
    dirty: bool,
    compiled: bool,
}

fn prepare_sources(
    mut sources: ResMut<Sources>,
    assets: Res<Assets<LootSource>>,
    server: Res<AssetServer>,
    registry: Res<LootRegistry>,
    mut events: MessageReader<AssetEvent<LootSource>>,
) {
    for event in events.read() {
        if sources.handles.iter().any(|handle| match event {
            AssetEvent::Added { id } => *id == handle.id() && !sources.compiled,
            AssetEvent::Modified { id } | AssetEvent::Removed { id } => *id == handle.id(),
            _ => false,
        }) {
            sources.dirty = true;
            if matches!(event, AssetEvent::Removed { .. }) {
                sources.compiled = false;
            }
        }
    }
    if !sources.dirty {
        return;
    }
    if sources.handles.is_empty() {
        sources.prepared = PreparedLoot::Failed(LootError::request("no loot sources configured"));
        sources.dirty = false;
        return;
    }
    for handle in &sources.handles {
        if assets.get(handle).is_none() {
            let path = server
                .get_path(handle.id())
                .expect("configured source has a path");
            sources.prepared = match server.load_state(handle.id()) {
                LoadState::Failed(error) => PreparedLoot::Failed(LootError::request(format!(
                    "could not load {path}: {error}"
                ))),
                LoadState::Loaded => PreparedLoot::Failed(LootError::request(format!(
                    "loot source {path} is unavailable"
                ))),
                _ => PreparedLoot::Loading,
            };
            sources.dirty = matches!(sources.prepared, PreparedLoot::Loading);
            return;
        }
    }
    let documents = sources.handles.iter().map(|handle| {
        let path = server
            .get_path(handle.id())
            .expect("configured source has a path");
        SourceDocument::new(
            path.to_string(),
            assets.get(handle).expect("source is loaded").text.clone(),
        )
    });
    sources.prepared = match registry.compile(documents) {
        Ok(catalog) => PreparedLoot::Ready(catalog),
        Err(error) => PreparedLoot::Failed(error),
    };
    // Added events can arrive after the source revision was already compiled, including failures.
    sources.compiled = true;
    sources.dirty = false;
}

/// A roll request, queued until source assets finish loading.
#[derive(Message)]
pub struct LootRequest<T: LootOutput> {
    id: RollId,
    table: String,
    inputs: BTreeMap<String, Value>,
    seed: u64,
    owner: Option<Entity>,
    #[cfg(test)]
    catalog: Option<Arc<Catalog>>,
    marker: std::marker::PhantomData<fn() -> T>,
}

impl<T: LootOutput> LootRequest<T> {
    /// Creates a request for the named table with a fresh roll ID and random seed.
    pub fn new(table: impl Into<String>) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("roll IDs exhausted");
        Self {
            id: RollId(id),
            table: table.into(),
            inputs: BTreeMap::new(),
            seed: fastrand::u64(..),
            owner: None,
            #[cfg(test)]
            catalog: None,
            marker: std::marker::PhantomData,
        }
    }
    /// Identifies the matching [`LootFinished`] message.
    pub fn id(&self) -> RollId {
        self.id
    }
    /// Sets the seed for table choices and generator randomness.
    /// Reproducing a result also requires unchanged sources, generators, and relevant world state.
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }
    /// Sets a named table input, replacing any previous value with that name.
    pub fn input(mut self, name: &str, value: impl LootValue) -> Self {
        self.inputs.insert(name.into(), value.into_value());
        self
    }
    /// Attaches an entity to [`LootFinished::owner`] without validating that it exists.
    pub fn owner(mut self, owner: Entity) -> Self {
        self.owner = Some(owner);
        self
    }

    #[cfg(test)]
    pub(crate) fn from_catalog(catalog: Arc<Catalog>, table: impl Into<String>, seed: u64) -> Self {
        let mut request = Self::new(table).seed(seed);
        request.catalog = Some(catalog);
        request
    }

    #[cfg(test)]
    pub(crate) fn from_table(table: crate::LootTable<T>, seed: u64) -> Self {
        Self::from_catalog(table.catalog, table.name, seed)
    }
}

/// Correlates a request with its completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RollId(u64);
/// A roll's completed bag or failure.
#[derive(Message, Clone, Debug)]
pub struct LootFinished<T: LootOutput> {
    /// Identifies the originating request.
    pub id: RollId,
    /// The request's owner, which may no longer exist.
    pub owner: Option<Entity>,
    /// The generated bag in evaluation order, or the error that stopped the roll.
    pub result: Result<Vec<T>, LootError>,
    #[cfg(test)]
    pub(crate) trace: Vec<crate::TraceEvent>,
}

#[derive(Resource, Default)]
struct OutputRegistrations(std::collections::HashSet<TypeId>);

#[derive(Message)]
struct NativeRequest {
    request: LootRequest<Value>,
    output_type: TypeId,
    validate: fn(&Catalog, &str) -> Result<(), LootError>,
}

#[derive(Message)]
struct NativeFinished {
    finished: LootFinished<Value>,
    output_type: TypeId,
}

fn relay_requests<T: LootOutput>(
    mut requests: MessageReader<LootRequest<T>>,
    mut native: MessageWriter<NativeRequest>,
) {
    for request in requests.read() {
        native.write(NativeRequest {
            request: LootRequest {
                id: request.id,
                table: request.table.clone(),
                inputs: request.inputs.clone(),
                seed: request.seed,
                owner: request.owner,
                #[cfg(test)]
                catalog: request.catalog.clone(),
                marker: std::marker::PhantomData,
            },
            output_type: TypeId::of::<T>(),
            validate: Catalog::validate_output::<T>,
        });
    }
}

fn relay_finished<T: LootOutput>(
    mut native: MessageReader<NativeFinished>,
    mut finished: MessageWriter<LootFinished<T>>,
) {
    for result in native
        .read()
        .filter(|result| result.output_type == TypeId::of::<T>())
    {
        finished.write(LootFinished {
            id: result.finished.id,
            owner: result.finished.owner,
            result: result
                .finished
                .result
                .clone()
                .and_then(crate::typed::convert_outputs::<T>),
            #[cfg(test)]
            trace: result.finished.trace.clone(),
        });
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Token {
    roll: RollId,
    index: usize,
}
#[derive(Message)]
struct NativeCall {
    token: Token,
    call: FunctionCall,
}
#[derive(Message)]
struct NativeResult {
    token: Token,
    generated: Result<Vec<Value>, String>,
}
struct Run {
    session: Session,
    owner: Option<Entity>,
    waiting: usize,
    output_type: TypeId,
}
#[derive(Resource, Default)]
struct Runtime {
    runs: HashMap<RollId, Run>,
    pending: VecDeque<NativeRequest>,
}
impl Runtime {
    fn current(&self, token: Token) -> bool {
        self.runs
            .get(&token.roll)
            .is_some_and(|run| run.waiting == token.index)
    }
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum LootSet {
    Evaluate,
    Generate,
    Publish,
}
/// Loads `.loot` assets and processes loot requests.
///
/// Requires [`bevy_asset::AssetPlugin`] to be installed first.
#[derive(Default)]
pub struct LootPlugin {
    sources: Vec<AssetPath<'static>>,
}
impl LootPlugin {
    /// Adds `.loot` asset paths to load. Tables can reference tables in other configured files.
    ///
    /// Requests fail if any configured file cannot be loaded or compiled, or if no files are configured.
    pub fn with_sources(
        mut self,
        sources: impl IntoIterator<Item = impl Into<AssetPath<'static>>>,
    ) -> Self {
        self.sources.extend(sources.into_iter().map(Into::into));
        self
    }
}
impl Plugin for LootPlugin {
    fn build(&self, app: &mut App) {
        assert!(
            !app.world().contains_resource::<Runtime>(),
            "LootPlugin was installed twice"
        );
        assert!(
            app.world().contains_resource::<bevy_asset::AssetServer>(),
            "LootPlugin requires AssetPlugin first"
        );
        app.init_resource::<LootRegistry>();
        app.init_asset::<LootSource>()
            .init_asset_loader::<LootLoader>()
            .init_resource::<Runtime>()
            .add_message::<NativeRequest>()
            .add_message::<NativeFinished>()
            .add_message::<NativeCall>()
            .add_message::<NativeResult>()
            .configure_sets(
                PostUpdate,
                (LootSet::Evaluate, LootSet::Generate, LootSet::Publish).chain(),
            )
            .add_systems(PreUpdate, prepare_sources)
            .add_systems(PostUpdate, drive.in_set(LootSet::Evaluate));
        let server = app.world().resource::<AssetServer>();
        let handles = self
            .sources
            .iter()
            .map(|path| server.load(path.clone()))
            .collect();
        app.insert_resource(Sources {
            handles,
            prepared: PreparedLoot::Loading,
            dirty: true,
            compiled: false,
        });
    }
}

fn finish(
    id: RollId,
    run: Run,
    result: Result<Vec<Value>, LootError>,
    finished: &mut MessageWriter<NativeFinished>,
) {
    finished.write(NativeFinished {
        output_type: run.output_type,
        finished: LootFinished {
            id,
            owner: run.owner,
            #[cfg(test)]
            trace: run.session.trace().to_vec(),
            result,
        },
    });
}

fn advance(
    id: RollId,
    mut run: Run,
    runtime: &mut Runtime,
    registry: &LootRegistry,
    native: &mut MessageWriter<NativeCall>,
    finished: &mut MessageWriter<NativeFinished>,
) {
    loop {
        let call = match run.session.advance() {
            Ok(Step::Call(call)) => call,
            Ok(Step::Finished(generated)) => {
                finish(id, run, Ok(generated), finished);
                return;
            }
            Err(error) => {
                finish(id, run, Err(error), finished);
                return;
            }
            Ok(Step::Waiting) => unreachable!("only contextual generators suspend a roll"),
        };
        match registry.generator(&call) {
            Ok(Generator::Pure(generator)) => {
                if let Err(error) = run.session.resume(invoke(&generator, call)) {
                    finish(id, run, Err(error), finished);
                    return;
                }
            }
            Ok(Generator::Contextual) => {
                run.waiting = call.index;
                native.write(NativeCall {
                    token: Token {
                        roll: id,
                        index: call.index,
                    },
                    call,
                });
                runtime.runs.insert(id, run);
                return;
            }
            Err(error) => {
                finish(id, run, Err(error), finished);
                return;
            }
        }
    }
}

#[derive(bevy_ecs::system::SystemParam)]
struct Ingress<'w, 's> {
    requests: MessageReader<'w, 's, NativeRequest>,
    results: MessageReader<'w, 's, NativeResult>,
    finished: MessageWriter<'w, NativeFinished>,
    native: MessageWriter<'w, NativeCall>,
}

fn drive(
    mut ingress: Ingress,
    mut runtime: ResMut<Runtime>,
    registry: Res<LootRegistry>,
    sources: Res<Sources>,
) {
    for result in ingress.results.read() {
        if !runtime.current(result.token) {
            continue;
        }
        let mut run = runtime
            .runs
            .remove(&result.token.roll)
            .expect("active roll");
        match run.session.resume(result.generated.clone()) {
            Ok(()) => advance(
                result.token.roll,
                run,
                &mut runtime,
                &registry,
                &mut ingress.native,
                &mut ingress.finished,
            ),
            Err(error) => finish(result.token.roll, run, Err(error), &mut ingress.finished),
        }
    }
    for native in ingress.requests.read() {
        runtime.pending.push_back(NativeRequest {
            request: LootRequest {
                id: native.request.id,
                table: native.request.table.clone(),
                inputs: native.request.inputs.clone(),
                seed: native.request.seed,
                owner: native.request.owner,
                #[cfg(test)]
                catalog: native.request.catalog.clone(),
                marker: std::marker::PhantomData,
            },
            output_type: native.output_type,
            validate: native.validate,
        });
    }
    for _ in 0..runtime.pending.len() {
        let native = runtime.pending.pop_front().expect("pending request exists");
        let request = &native.request;
        let id = request.id;
        #[cfg(test)]
        let prepared = request
            .catalog
            .as_ref()
            .map(|catalog| PreparedLoot::Ready(catalog.clone()));
        #[cfg(test)]
        let prepared = prepared.as_ref().unwrap_or(&sources.prepared);
        #[cfg(not(test))]
        let prepared = &sources.prepared;
        let catalog = match prepared {
            PreparedLoot::Ready(catalog) => Ok(catalog.clone()),
            PreparedLoot::Loading => {
                runtime.pending.push_back(native);
                continue;
            }
            PreparedLoot::Failed(error) => Err(error.clone()),
        };
        let session = catalog
            .and_then(|catalog| {
                (native.validate)(&catalog, &request.table)?;
                Session::new(
                    catalog,
                    &request.table,
                    request.inputs.clone(),
                    request.seed,
                )
            })
            .map(|session| session.with_limits(Limits::default()));
        match session {
            Ok(session) => advance(
                id,
                Run {
                    session,
                    owner: request.owner,
                    waiting: 0,
                    output_type: native.output_type,
                },
                &mut runtime,
                &registry,
                &mut ingress.native,
                &mut ingress.finished,
            ),
            Err(error) => {
                ingress.finished.write(NativeFinished {
                    output_type: native.output_type,
                    finished: LootFinished {
                        id,
                        owner: request.owner,
                        result: Err(error),
                        #[cfg(test)]
                        trace: Vec::new(),
                    },
                });
            }
        }
    }
}
