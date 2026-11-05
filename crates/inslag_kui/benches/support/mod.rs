//! Public-API fixtures: KUI is always owned by an authored SCXML activity.
#![allow(dead_code)]

use std::{
    collections::HashMap,
    path::Path,
    time::{Duration, Instant},
};

use bevy::{
    asset::io::{
        AssetSourceBuilder, AssetSourceId,
        memory::{Dir, MemoryAssetReader},
    },
    ecs::system::SystemId,
    prelude::*,
};
use bevy_flair::prelude::{AttributeList, ClassList, StyleSheet, Styled, TypeName};
use inslag_kui::state_tree::KuiStateTreePlugin;
use inslag_state_tree::{
    EntityStateTreeEventWriter, LuauValue, StateTreeDefinition, StateTreeEventDataError,
    StateTreeExt, StateTreeInstance, StateTreePlugin, StateTreeSet, scxml,
};

const HEADER: &str = r#"<view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="benchmark">"#;
const CSS: &str = ".cell { width: 1px; height: 1px; background-color: white; }";
const COMPONENT: &str = r#"<component xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="cell"><prop name="value" type="scalar"/><node class="cell" bind:attr-value="props.value"><slot name="detail"/></node></component>"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Native,
    Flair,
    Scxml,
}
pub trait Choice {
    const BACKEND: Backend;
}
pub struct Native;
pub struct Flair;
pub struct Scxml;
impl Choice for Native {
    const BACKEND: Backend = Backend::Native;
}
impl Choice for Flair {
    const BACKEND: Backend = Backend::Flair;
}
impl Choice for Scxml {
    const BACKEND: Backend = Backend::Scxml;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pipeline {
    Runtime,
    Cpu,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Static,
    Scalar,
    Sparse,
    Expression,
    Shared,
    Repeat,
    RepeatExternal,
    Component,
    Text,
    Focus,
    MixedFocus,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    Idle,
    Toggle,
    Sparse,
    Unrelated,
    Reorder,
    Churn,
    Next,
    Previous,
    Activate,
    Burst,
    Restart,
    CancelReenter,
    StaleGeneration,
    Reload,
    RestartTarget,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub backend: Backend,
    pub pipeline: Pipeline,
    pub scene: Scene,
    pub work: Work,
    pub views: usize,
    pub controllers: usize,
    pub nodes: usize,
    pub payload: usize,
    pub burst: usize,
}
impl Config {
    pub fn new(backend: Backend, scene: Scene, nodes: usize) -> Self {
        Self {
            backend,
            pipeline: Pipeline::Runtime,
            scene,
            work: Work::Idle,
            views: 1,
            controllers: 1,
            nodes,
            payload: 0,
            burst: 1,
        }
    }
    pub fn protocol(views: usize, controllers: usize, nodes: usize, work: Work) -> Self {
        let mut config = Self::new(Backend::Scxml, Scene::Focus, nodes);
        config.views = views;
        config.controllers = controllers;
        config.work = work;
        config
    }
    pub fn elements(&self) -> usize {
        self.nodes * self.views
    }
    fn per_controller(&self) -> usize {
        self.views / self.controllers
    }
    fn target(&self) -> String {
        format!("v{}", self.per_controller() - 1)
    }
}

// This event belongs to the BENCHMARK APPLICATION, not to KUI. The chart handles
// it and issues the same effects an application would issue for device input.
#[derive(EntityEvent, inslag_state_tree::StateTreeEvent)]
#[state_tree(name = "bench.input", data = input_data)]
struct BenchInput {
    entity: Entity,
    operation: &'static str,
    view: usize,
}
fn input_data(event: &BenchInput) -> Result<LuauValue, StateTreeEventDataError> {
    Ok(LuauValue::Record(
        [
            (
                "operation".into(),
                LuauValue::String(event.operation.into()),
            ),
            ("view".into(), LuauValue::String(format!("v{}", event.view))),
        ]
        .into(),
    ))
}
#[derive(Message)]
struct ReadySeen {
    controller: Entity,
    view: String,
}
#[derive(Resource, Default)]
struct ReadyLedger(HashMap<Entity, Vec<usize>>);
fn record_ready(mut messages: MessageReader<ReadySeen>, mut ledger: ResMut<ReadyLedger>) {
    for message in messages.read() {
        let index: usize = message
            .view
            .strip_prefix('v')
            .expect("unknown authored view ID")
            .parse()
            .unwrap();
        ledger
            .0
            .get_mut(&message.controller)
            .expect("ready routed to wrong controller")[index] += 1;
    }
}
struct SendInput {
    entity: Entity,
    operation: &'static str,
    view: usize,
    count: usize,
}
fn send_input(In(input): In<SendInput>, mut events: EntityStateTreeEventWriter) {
    for _ in 0..input.count {
        assert!(events.write(BenchInput {
            entity: input.entity,
            operation: input.operation,
            view: input.view
        }));
    }
}

pub struct Fixture {
    app: App,
    pub config: Config,
    directory: Dir,
    input: SystemId<In<SendInput>>,
    controllers: Vec<Entity>,
    roots: Vec<Entity>,
    leaves: Vec<Entity>,
    target_root: Entity,
    generation: usize,
    steps: usize,
    actions: usize,
    expected_ready: usize,
    initial_leaves: Vec<Entity>,
}

impl Fixture {
    pub fn new(config: Config) -> Self {
        assert!(config.nodes > 0 && config.views > 0 && config.controllers > 0);
        assert!(config.views.is_multiple_of(config.controllers));
        if matches!(config.scene, Scene::Repeat | Scene::RepeatExternal) {
            assert!(
                config.nodes <= 1_000,
                "split large collections into 1,000-row views to stay below Luau conversion limits"
            );
        }
        let directory = Dir::default();
        directory.insert_asset_text(Path::new("bench.css"), CSS);
        directory.insert_asset_text(Path::new("cell.kui"), COMPONENT);
        for (path, target) in [("bench.kui", false), ("target.kui", true)] {
            let source = source(&config, target, 0);
            inslag_kui::source::parse(path, &source).expect("invalid authored benchmark fixture");
            directory.insert_asset_text(Path::new(path), &source);
        }
        let mut app = App::new();
        let reader = directory.clone();
        app.register_asset_source(
            AssetSourceId::Default,
            AssetSourceBuilder::new(move || {
                Box::new(MemoryAssetReader {
                    root: reader.clone(),
                })
            }),
        );
        match config.pipeline {
            Pipeline::Runtime => {
                app.add_plugins((
                    MinimalPlugins,
                    AssetPlugin {
                        watch_for_changes_override: Some(false),
                        use_asset_processor_override: Some(false),
                        ..default()
                    },
                ));
                app.init_asset::<Image>();
            }
            Pipeline::Cpu => {
                app.add_plugins(AssetPlugin {
                    watch_for_changes_override: Some(false),
                    use_asset_processor_override: Some(false),
                    ..default()
                });
                app.init_asset::<bevy::shader::Shader>();
                app.add_plugins(
                    DefaultPlugins
                        .build()
                        .disable::<bevy::log::LogPlugin>()
                        .disable::<bevy::winit::WinitPlugin>()
                        .disable::<bevy::render::RenderPlugin>()
                        .disable::<AssetPlugin>()
                        .set(WindowPlugin {
                            primary_window: None,
                            exit_condition: bevy::window::ExitCondition::DontExit,
                            ..default()
                        }),
                );
            }
        }
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f64(1.0 / 60.0),
        ));
        if config.backend == Backend::Flair
            || (config.pipeline == Pipeline::Cpu && config.backend == Backend::Scxml)
        {
            app.add_plugins(bevy_flair::FlairPlugin);
        }
        // Native/Flair controls intentionally include neither BST nor KUI. They
        // are renderer cost floors, NOT equivalent protocol implementations.
        if config.backend == Backend::Scxml {
            app.add_plugins((StateTreePlugin::new(Update), KuiStateTreePlugin))
                .add_state_tree_event::<BenchInput>()
                .add_state_tree_effect::<ReadySeen, _>("bench.ready", |context, args| {
                    Ok(ReadySeen {
                        controller: context.entity(),
                        view: args.take::<String>("view")?,
                    })
                })
                .init_resource::<ReadyLedger>()
                .add_systems(Update, record_ready.in_set(StateTreeSet::HandleEffects));
        }
        app.finish();
        app.cleanup();
        let input = app.world_mut().register_system(send_input);
        let mut fixture = Self {
            app,
            config,
            directory,
            input,
            controllers: vec![],
            roots: vec![],
            leaves: vec![],
            target_root: Entity::PLACEHOLDER,
            generation: 0,
            steps: 0,
            actions: 0,
            expected_ready: 0,
            initial_leaves: vec![],
        };
        fixture.spawn();
        if fixture.config.backend == Backend::Scxml {
            fixture.expected_ready = fixture.config.views;
            fixture.wait_ready();
            fixture.collect();
        }
        if fixture.config.pipeline == Pipeline::Cpu {
            fixture.prepare_layout();
        }
        for _ in 0..8 {
            fixture.app.update();
        }
        fixture.initial_leaves = fixture.leaves.clone();
        fixture.verify();
        // Exercise both sides of every alternating workload before measuring.
        fixture.step();
        fixture.verify();
        fixture.step();
        fixture.verify();
        fixture
    }

    fn spawn(&mut self) {
        if self.config.backend == Backend::Scxml {
            let chart = chart(&self.config);
            let definition =
                scxml::compile(&scxml::parse("benchmark.scxml", &chart).unwrap()).unwrap();
            let handle = self
                .app
                .world_mut()
                .resource_mut::<Assets<StateTreeDefinition>>()
                .add(definition);
            for _ in 0..self.config.controllers {
                let controller = self
                    .app
                    .world_mut()
                    .spawn(StateTreeInstance::new(handle.clone()))
                    .id();
                self.controllers.push(controller);
                self.app
                    .world_mut()
                    .resource_mut::<ReadyLedger>()
                    .0
                    .insert(controller, vec![0; self.config.per_controller()]);
            }
        } else {
            let style: Option<Handle<StyleSheet>> = (self.config.backend == Backend::Flair)
                .then(|| self.app.world().resource::<AssetServer>().load("bench.css"));
            for _ in 0..self.config.views {
                let root = self.app.world_mut().spawn(Node::default()).id();
                if let Some(style) = &style {
                    self.app
                        .world_mut()
                        .entity_mut(root)
                        .insert((Styled::new(style.clone()), TypeName("node")));
                }
                self.roots.push(root);
                for _ in 0..self.config.nodes {
                    let mut leaf = self.app.world_mut().spawn((
                        Node::default(),
                        ChildOf(root),
                        Pickable::IGNORE,
                    ));
                    if self.config.scene == Scene::Text {
                        leaf.insert(Text::new("0"));
                    } else if self.config.scene != Scene::Static {
                        let mut attributes = AttributeList::new();
                        attributes.set_attribute("value", "0");
                        leaf.insert(attributes);
                    }
                    if self.config.backend == Backend::Flair {
                        leaf.insert((Styled::default(), ClassList::new("cell")));
                        if self.config.scene != Scene::Text {
                            leaf.insert(TypeName("node"));
                        }
                    }
                    self.leaves.push(leaf.id());
                }
            }
        }
    }

    #[cfg_attr(
        dylint_lib = "inslag_state_tree_private_access",
        allow(
            inslag_state_tree_private_access,
            reason = "benchmark assertions verify authored chart counters"
        )
    )]
    fn number(&self, owner: Entity, name: &str) -> usize {
        match self
            .app
            .world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .data(name)
            .unwrap()
        {
            LuauValue::Number(value) => *value as usize,
            LuauValue::Integer(value) => *value as usize,
            value => panic!("expected numeric chart counter {name}, got {value:?}"),
        }
    }
    fn total(&self, name: &str) -> usize {
        self.controllers
            .iter()
            .map(|owner| self.number(*owner, name))
            .sum()
    }
    fn wait_ready(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            self.app.update();
            let initialized = self.controllers.iter().all(|owner| {
                if let Some(failure) = self
                    .app
                    .world()
                    .get::<inslag_state_tree::StateTreeLinkFailure>(*owner)
                {
                    panic!("benchmark chart did not link: {:?}", failure.error());
                }
                match self
                    .app
                    .world()
                    .get::<StateTreeInstance>(*owner)
                    .unwrap()
                    .status()
                {
                    inslag_state_tree::InstanceStatus::Pending => false,
                    inslag_state_tree::InstanceStatus::Running => true,
                    status => panic!("benchmark controller failed to remain running: {status:?}"),
                }
            });
            if initialized {
                assert_eq!(
                    self.total("errors"),
                    0,
                    "chart observed an activity/protocol error"
                );
                if self.total("ready") == self.expected_ready {
                    break;
                }
            }
            assert!(
                Instant::now() < deadline,
                "missing ui.render.ready delivery: expected {} (controllers initialized: {initialized})",
                self.expected_ready
            );
            std::thread::yield_now();
        }
    }
    fn collect(&mut self) {
        self.roots.clear();
        self.leaves.clear();
        for controller in &self.controllers {
            let mounts = self
                .app
                .world()
                .get::<Children>(*controller)
                .expect("controller -> mount hierarchy")
                .to_vec();
            assert_eq!(
                mounts.len(),
                self.config.per_controller(),
                "unexpected controller children: {mounts:?}"
            );
            for mount in mounts {
                let roots = self
                    .app
                    .world()
                    .get::<Children>(mount)
                    .expect("mount -> root hierarchy");
                assert_eq!(roots.len(), 1);
                let root = roots[0];
                if *controller == self.controllers[0]
                    && self.app.world().get::<Name>(root).unwrap().as_str() == "target-root"
                {
                    self.target_root = root;
                }
                self.roots.push(root);
                self.leaves
                    .extend(self.app.world().get::<Children>(root).unwrap().iter());
            }
        }
        assert_eq!(self.roots.len(), self.config.views);
        assert_eq!(
            self.leaves.len(),
            self.config.elements(),
            "empty/partial collection must not benchmark as success"
        );
        assert_ne!(self.target_root, Entity::PLACEHOLDER);
    }
    fn send(&mut self, operation: &'static str, all: bool, count: usize) {
        let owners = if all { self.controllers.len() } else { 1 };
        for index in 0..owners {
            self.app
                .world_mut()
                .run_system_with(
                    self.input,
                    SendInput {
                        entity: self.controllers[index],
                        operation,
                        view: self.config.per_controller() - 1,
                        count,
                    },
                )
                .unwrap();
        }
    }

    pub fn step(&mut self) {
        self.steps += 1;
        match self.config.work {
            Work::Idle => self.app.update(),
            Work::Next | Work::Previous | Work::Activate | Work::Burst => {
                let operation = match self.config.work {
                    Work::Previous => "previous",
                    Work::Activate => "activate",
                    _ => "next",
                };
                let count = if self.config.work == Work::Burst {
                    self.config.burst
                } else {
                    1
                };
                // Focus is established through an authored input, never by
                // assigning InputFocus. The activate fixture's initial focus is
                // prepared lazily once, outside its measured repeated path.
                if self.config.work == Work::Activate && self.actions == 0 {
                    self.send("next", false, 1);
                    self.app.update();
                }
                self.send(operation, false, count);
                self.app.update();
                if self.config.work == Work::Activate {
                    self.actions += count;
                }
            }
            Work::Restart | Work::RestartTarget | Work::CancelReenter | Work::StaleGeneration => {
                let old: Vec<_> = self
                    .roots
                    .iter()
                    .map(|root| {
                        (
                            *root,
                            self.app.world().get::<Name>(*root).unwrap().as_str() == "target-root",
                        )
                    })
                    .collect();
                if self.config.work == Work::CancelReenter {
                    self.send("cancel", true, 1);
                    self.app.update();
                    for (root, _) in &old {
                        assert!(!self.app.world().entities().contains(*root));
                    }
                    self.send("resume", true, 1);
                } else {
                    if self.config.work == Work::StaleGeneration {
                        // Capture real old-generation effect tokens while the
                        // activity is active, then cancel before HandleEffects.
                        self.send("next", true, 1);
                        self.send("activate", true, 1);
                    }
                    let operation = match self.config.work {
                        Work::RestartTarget => "remount-target",
                        _ => "restart",
                    };
                    self.send(operation, true, 1);
                }
                self.expected_ready += if self.config.work == Work::RestartTarget {
                    self.config.controllers
                } else {
                    self.config.views
                };
                self.wait_ready();
                for (root, target) in old {
                    let replaced = self.config.work != Work::RestartTarget || target;
                    assert_eq!(
                        self.app.world().entities().contains(root),
                        !replaced,
                        "wrong scope was replaced by lifecycle churn"
                    );
                }
                self.collect();
            }
            Work::Reload => {
                self.generation = 1 - self.generation;
                let old = self.target_root;
                let unrelated: Vec<_> = self
                    .roots
                    .iter()
                    .copied()
                    .filter(|root| {
                        self.app.world().get::<Name>(*root).unwrap().as_str() != "target-root"
                    })
                    .collect();
                self.directory.insert_asset_text(
                    Path::new("target.kui"),
                    &source(&self.config, true, self.generation),
                );
                self.app
                    .world()
                    .resource::<AssetServer>()
                    .reload("target.kui");
                self.expected_ready += self.config.controllers;
                let deadline = Instant::now() + Duration::from_secs(120);
                loop {
                    self.app.update();
                    if !self.app.world().entities().contains(old) {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "public asset reload did not replace the hierarchy"
                    );
                    std::thread::yield_now();
                }
                self.wait_ready();
                for root in unrelated {
                    assert!(
                        self.app.world().entities().contains(root),
                        "reload replaced an unrelated view"
                    );
                }
                self.collect();
            }
            work => {
                self.generation = 1 - self.generation;
                if self.config.backend == Backend::Scxml {
                    let operation = match work {
                        Work::Sparse => "sparse",
                        Work::Unrelated => "unrelated",
                        Work::Reorder => "reorder",
                        Work::Churn => "churn",
                        _ => "toggle",
                    };
                    self.send(operation, work != Work::Sparse, 1);
                } else {
                    let expected = if self.generation == 0 { "0" } else { "1" };
                    let count = if work == Work::Sparse {
                        1
                    } else if work == Work::Unrelated {
                        0
                    } else {
                        self.leaves.len()
                    };
                    for leaf in self.leaves.iter().take(count) {
                        if self.config.scene == Scene::Text {
                            self.app.world_mut().get_mut::<Text>(*leaf).unwrap().0 =
                                expected.into();
                        } else {
                            self.app
                                .world_mut()
                                .get_mut::<AttributeList>(*leaf)
                                .unwrap()
                                .set_attribute("value", expected);
                        }
                    }
                }
                self.app.update();
                if self.config.work == Work::Churn {
                    self.collect();
                }
            }
        }
    }

    /// Verification and pending protocol-event delivery are outside timing.
    #[cfg_attr(
        dylint_lib = "inslag_state_tree_private_access",
        allow(
            inslag_state_tree_private_access,
            reason = "benchmark assertions verify authored chart behavior"
        )
    )]
    pub fn verify(&mut self) {
        for _ in 0..2 {
            self.app.update();
        }
        if self.config.backend == Backend::Scxml {
            assert_eq!(self.total("errors"), 0);
            assert_eq!(self.total("ready"), self.expected_ready);
            for controller in &self.controllers {
                let delivered = &self.app.world().resource::<ReadyLedger>().0[controller];
                assert_eq!(delivered.len(), self.config.per_controller());
                for (index, count) in delivered.iter().copied().enumerate() {
                    let expected = if matches!(self.config.work, Work::Reload | Work::RestartTarget)
                    {
                        if index + 1 == self.config.per_controller() {
                            self.steps + 1
                        } else {
                            1
                        }
                    } else {
                        self.expected_ready / self.config.views
                    };
                    assert_eq!(
                        count, expected,
                        "missing or duplicate ready notification for authored view v{index}"
                    );
                }
            }
            assert_eq!(
                self.total("actions"),
                self.actions,
                "activation must produce exactly one real ui.action per input"
            );
            let expected_focus_changes = match self.config.work {
                Work::Next | Work::Previous => self.steps,
                Work::Burst => self.steps * self.config.burst,
                Work::Activate if self.steps > 0 => 1,
                _ => 0,
            };
            assert_eq!(
                self.total("focus_changes"),
                expected_focus_changes,
                "focus event delivery or generation isolation failed"
            );
            if self.config.work == Work::Unrelated {
                for owner in &self.controllers {
                    assert_eq!(
                        self.number(*owner, "changed"),
                        self.generation,
                        "domain event did not execute"
                    );
                }
            }
            if matches!(self.config.work, Work::Activate) && self.actions > 0 {
                let tree = self
                    .app
                    .world()
                    .get::<StateTreeInstance>(self.controllers[0])
                    .unwrap();
                assert_eq!(
                    tree.data("last_view"),
                    Some(&LuauValue::String(self.config.target()))
                );
                assert_eq!(
                    tree.data("last_value"),
                    Some(&LuauValue::String("0".into()))
                );
            }
            if matches!(
                self.config.work,
                Work::Next | Work::Previous | Work::Burst | Work::Activate
            ) && self.steps > 0
            {
                let focused = self
                    .app
                    .world()
                    .resource::<bevy::input_focus::InputFocus>()
                    .get()
                    .expect("authored ui.focus did not move focus");
                let children = self.app.world().get::<Children>(self.target_root).unwrap();
                assert!(
                    children.contains(&focused),
                    "navigation escaped its activity/controller scope"
                );
                let eligible: Vec<_> = children
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| self.config.scene != Scene::MixedFocus || index % 4 < 2)
                    .map(|(_, entity)| entity)
                    .collect();
                let requests = self.steps
                    * if self.config.work == Work::Burst {
                        self.config.burst
                    } else {
                        1
                    };
                let index = match self.config.work {
                    Work::Activate => 0,
                    Work::Previous => (eligible.len() - requests % eligible.len()) % eligible.len(),
                    _ => (requests - 1) % eligible.len(),
                };
                assert_eq!(
                    focused, eligible[index],
                    "next/previous order, wraparound, or hidden/disabled filtering is wrong"
                );
                let tree = self
                    .app
                    .world()
                    .get::<StateTreeInstance>(self.controllers[0])
                    .unwrap();
                assert_eq!(
                    tree.data("last_focus"),
                    Some(&LuauValue::String(self.config.target()))
                );
            }
        }
        let world = self.app.world_mut();
        assert_eq!(
            world.query::<&Node>().iter(world).count(),
            self.config.elements() + self.config.views
        );
        if !matches!(
            self.config.work,
            Work::Restart
                | Work::RestartTarget
                | Work::CancelReenter
                | Work::StaleGeneration
                | Work::Reload
                | Work::Churn
        ) {
            for leaf in &self.initial_leaves {
                assert!(world.entities().contains(*leaf), "unexpected entity churn");
            }
        }
        if !matches!(
            self.config.scene,
            Scene::Focus | Scene::MixedFocus | Scene::Static
        ) {
            for (index, leaf) in self.leaves.iter().enumerate() {
                if self.config.work == Work::Churn && !world.entities().contains(*leaf) {
                    continue;
                }
                let changed = match self.config.work {
                    Work::Toggle => true,
                    Work::Sparse => {
                        index < self.config.per_controller() * self.config.nodes
                            && index.is_multiple_of(self.config.nodes)
                    }
                    _ => false,
                };
                let expected = if changed && self.generation == 1 {
                    "1"
                } else {
                    "0"
                };
                if self.config.scene == Scene::Text {
                    assert_eq!(world.get::<Text>(*leaf).unwrap().0, expected);
                } else {
                    assert_eq!(
                        world
                            .get::<AttributeList>(*leaf)
                            .unwrap()
                            .get_attribute("value"),
                        Some(expected)
                    );
                }
            }
        }
        if self.config.backend == Backend::Scxml
            && matches!(self.config.scene, Scene::Repeat | Scene::RepeatExternal)
        {
            for root in &self.roots {
                let children = world.get::<Children>(*root).unwrap();
                assert_eq!(children.len(), self.config.nodes);
                for (index, entity) in children.iter().enumerate() {
                    let key = if self.config.work == Work::Reorder && self.generation == 1 {
                        self.config.nodes - 1 - index
                    } else if self.config.work == Work::Churn && index + 1 == self.config.nodes {
                        index + self.generation
                    } else {
                        index
                    };
                    assert_eq!(
                        world
                            .get::<AttributeList>(entity)
                            .unwrap()
                            .get_attribute("key"),
                        Some(key.to_string().as_str()),
                        "keyed reconciliation produced the wrong order/generation"
                    );
                }
            }
        }
        if self.config.work == Work::Reload {
            assert_eq!(
                world
                    .get::<AttributeList>(self.target_root)
                    .unwrap()
                    .get_attribute("generation"),
                Some(self.generation.to_string().as_str()),
                "reload did not consume changed source"
            );
        }
        if self.config.pipeline == Pipeline::Cpu {
            for leaf in &self.leaves {
                assert!(
                    world.get::<ComputedNode>(*leaf).unwrap().size().x > 0.0,
                    "CPU layout did not run"
                );
                if self.config.scene == Scene::Text {
                    assert!(
                        !world
                            .get::<bevy::text::TextLayoutInfo>(*leaf)
                            .expect("CPU text layout was not installed")
                            .glyphs
                            .is_empty(),
                        "CPU text did not shape any glyphs"
                    );
                }
            }
        }
    }
    pub fn single_threaded(&mut self) {
        assert_eq!(
            bevy::tasks::ComputeTaskPool::get().thread_num(),
            0,
            "allocation fixtures require a caller-only compute pool, not just a serial scheduler"
        );
        use bevy::ecs::schedule::SingleThreadedExecutor;
        macro_rules! serial { ($($schedule:expr),*) => { $(self.app.edit_schedule($schedule, |s| { s.set_executor(SingleThreadedExecutor::new()); });)* }; }
        serial!(First, PreUpdate, Update, PostUpdate, Last);
        for _ in 0..4 {
            self.app.update();
        }
    }
    fn prepare_layout(&mut self) {
        let camera = self
            .app
            .world_mut()
            .spawn((
                Camera2d,
                Camera {
                    computed: bevy::camera::ComputedCameraValues {
                        target_info: Some(bevy::camera::RenderTargetInfo {
                            physical_size: UVec2::new(1280, 900),
                            scale_factor: 1.0,
                        }),
                        ..default()
                    },
                    ..default()
                },
                bevy::camera::RenderTarget::None {
                    size: UVec2::new(1280, 900),
                },
            ))
            .id();
        for root in &self.roots {
            self.app.world_mut().entity_mut(*root).insert((
                UiTargetCamera(camera),
                Node {
                    width: Val::Px(1280.0),
                    height: Val::Px(900.0),
                    position_type: PositionType::Absolute,
                    ..default()
                },
            ));
        }
        for (index, leaf) in self.leaves.iter().enumerate() {
            self.app.world_mut().entity_mut(*leaf).insert((
                Node {
                    width: Val::Px(1.0),
                    height: Val::Px(1.0),
                    position_type: PositionType::Absolute,
                    left: Val::Px((index % 1280) as f32),
                    top: Val::Px(((index / 1280) % 900) as f32),
                    ..default()
                },
                BackgroundColor(Color::WHITE),
            ));
        }
    }
}

fn source(config: &Config, target: bool, generation: usize) -> String {
    let mut source = HEADER.to_owned();
    if config.pipeline == Pipeline::Cpu {
        source.push_str(r#"<style src="bench.css"/>"#);
    }
    if config.scene == Scene::Component {
        source.push_str(r#"<import name="cell" src="cell.kui"/>"#);
    }
    source.push_str(&format!(
        r#"<node id="{}" bind:attr-generation="{generation}">"#,
        if target { "target-root" } else { "root" }
    ));
    match config.scene {
        Scene::Repeat | Scene::RepeatExternal => source.push_str(if config.scene == Scene::Repeat {
            r#"<repeat bind:items="rows" as="row" key="id"><node class="cell" bind:attr-value="row.value" bind:attr-key="row.id"/></repeat>"#
        } else { r#"<repeat bind:items="rows" as="row" key="id"><node class="cell" bind:attr-value="row.value + value" bind:attr-key="row.id"/></repeat>"# }),
        _ => for index in 0..config.nodes {
            match config.scene {
                Scene::Static => source.push_str(r#"<node class="cell"/>"#),
                Scene::Scalar => source.push_str(r#"<node class="cell" bind:attr-value="value"/>"#),
                Scene::Sparse => source.push_str(if index == 0 { r#"<node class="cell" bind:attr-value="changed"/>"# } else { r#"<node class="cell" bind:attr-value="value"/>"# }),
                Scene::Expression => source.push_str(r#"<node class="cell" bind:attr-value="value + 0"/>"#),
                Scene::Shared => source.push_str(r#"<node class="cell" bind:attr-value="model.value"/>"#),
                Scene::Component => source.push_str(r#"<use component="cell"><prop name="value" bind:value="value"/><fill slot="detail"></fill></use>"#),
                Scene::Text => source.push_str(r#"<text class="cell" bind:text="value"/>"#),
                Scene::Focus | Scene::MixedFocus => {
                    let availability = if config.scene == Scene::MixedFocus { match index % 4 {
                        2 => r#" bind:disabled="true""#, 3 => r#" bind:visible="false""#, _ => "",
                    }} else { "" };
                    source.push_str(&format!(r#"<button on-click="choose" value="{index}"{availability}/>"#));
                }
                Scene::Repeat | Scene::RepeatExternal => unreachable!(),
            }
        },
    }
    source.push_str("</node></view>");
    source
}

fn chart(config: &Config) -> String {
    let mut activities = String::new();
    let mut target_activity = String::new();
    let mut activity_errors = String::new();
    for index in 0..config.per_controller() {
        let target = index + 1 == config.per_controller();
        let activity = format!(
            r#"<bst:activity id="v{index}" name="ui.render"><bst:arg name="src" expr="'{}'"/><bst:arg name="space" expr="'layout'"/></bst:activity>"#,
            if target { "target.kui" } else { "bench.kui" }
        );
        if target {
            target_activity.push_str(&activity);
        } else {
            activities.push_str(&activity);
        }
        activity_errors.push_str(&format!(" error.activity.v{index}"));
    }
    // Explicit row literals avoid depending on application VM globals/builtins.
    let rows = if matches!(config.scene, Scene::Repeat | Scene::RepeatExternal) {
        (0..config.nodes)
            .map(|index| format!("{{id={index},value=0}},"))
            .collect::<String>()
    } else {
        String::new()
    };
    let payload = std::iter::repeat_n("7,", config.payload).collect::<String>();
    let toggle = match config.scene {
        Scene::Shared => "model.value",
        _ => "value",
    };
    format!(
        r##"<scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:bst="urn:inslag-state-tree" datamodel="luau" initial="active">
      <datamodel>
        <data id="value" expr="0"/><data id="changed" expr="0"/>
        <data id="model" expr="{{value=0,payload={{{payload}}}}}"/>
        <data id="rows" expr="{{{rows}}}"/>
        <data id="ready" expr="0"/>
        <data id="actions" expr="0"/><data id="focus_changes" expr="0"/><data id="errors" expr="0"/>
        <data id="last_view" expr="''"/><data id="last_value" expr="''"/><data id="last_focus" expr="''"/>
      </datamodel>
      <state id="active" initial="target">{activities}
        <transition event="ui.render.ready"><assign location="ready" expr="ready + 1"/><bst:effect name="bench.ready"><bst:arg name="view" expr="_event.data.view"/></bst:effect></transition>
        <transition event="ui.focus.changed"><assign location="focus_changes" expr="focus_changes + 1"/><assign location="last_focus" expr="_event.data.view"/></transition>
        <transition event="ui.action" cond="_event.data.action == 'choose' and _event.data.position == nil and _event.data.start == nil"><assign location="actions" expr="actions + 1"/><assign location="last_view" expr="_event.data.view"/><assign location="last_value" expr="_event.data.value"/></transition>
        <transition event="ui.render.ready ui.focus.changed ui.action{activity_errors}"><assign location="errors" expr="errors + 1"/></transition>
        <transition event="bench.input" cond="_event.data.operation == 'next' or _event.data.operation == 'previous'"><bst:effect name="ui.focus"><bst:arg name="view" expr="_event.data.view"/><bst:arg name="direction" expr="_event.data.operation"/></bst:effect></transition>
        <transition event="bench.input" cond="_event.data.operation == 'activate'"><bst:effect name="ui.activate"><bst:arg name="view" expr="_event.data.view"/></bst:effect></transition>
        <transition event="bench.input" cond="_event.data.operation == 'toggle'"><assign location="{toggle}" expr="1 - {toggle}"/></transition>
        <transition event="bench.input" cond="_event.data.operation == 'sparse'"><assign location="changed" expr="1 - changed"/>{sparse_row}</transition>
        <transition event="bench.input" cond="_event.data.operation == 'unrelated'"><assign location="changed" expr="1 - changed"/></transition>
        <transition event="bench.input" cond="_event.data.operation == 'reorder'"><assign location="rows" expr="(function() local result = {{}}; for i = #rows, 1, -1 do result[#result + 1] = rows[i] end; return result end)()"/></transition>
        <transition event="bench.input" cond="_event.data.operation == 'churn'"><assign location="changed" expr="1 - changed"/><assign location="rows[#rows].id" expr="#rows - 1 + changed"/></transition>
        <transition event="bench.input" cond="_event.data.operation == 'restart'" target="active"/>
        <transition event="bench.input" cond="_event.data.operation == 'cancel'" target="inactive"/>
        <state id="target">{target_activity}<transition event="bench.input" cond="_event.data.operation == 'remount-target'" target="target"/></state>
      </state>
      <state id="inactive"><transition event="bench.input" cond="_event.data.operation == 'resume'" target="active"/></state>
    </scxml>"##,
        sparse_row = if config.scene == Scene::Repeat {
            r#"<assign location="rows[1].value" expr="changed"/>"#
        } else {
            ""
        }
    )
}
