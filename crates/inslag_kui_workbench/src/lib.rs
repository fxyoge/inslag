//! Preview KUI assets, edit sample data, and inspect interactions in a standalone window.
//!
//! ```no_run
//! let mut app = inslag_kui_workbench::workbench_app("assets");
//! app.run();
//! ```
//!
//! Choose a view, then click its data fields to edit the preview. Asset changes
//! reload automatically. Enable `aseprite` for views that use Aseprite slices.
//!
//! Use [`workbench_app_with_stories`] to keep named sample datasets alongside your project.
mod data;
mod editor;
mod fixtures;
mod preview;
mod stories;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use bevy::{
    asset::io::{
        AssetSourceBuilder, AssetSourceId,
        file::FileAssetReader,
        memory::{Dir, MemoryAssetReader},
    },
    prelude::*,
};
use bevy_enhanced_input::prelude::*;
use bevy_flair::FlairPlugin;
use data::{Binding, Data, Kind};
use inslag_kui::{source as kui, state_tree::KuiStateTreePlugin};
use inslag_state_tree::{LuauValue as KuiValue, StateTreeInstance, StateTreePlugin, StateTreeSet};

/// Open a workbench for `.kui` files under `asset_root`.
///
/// Asset paths in views and styles are relative to this directory. Add any
/// project-specific loaders or rendering plugins to the returned app before `run`.
pub fn workbench_app(asset_root: impl Into<PathBuf>) -> App {
    configured_app(asset_root.into(), None)
}

/// Open a workbench with named sample datasets from `story_root`.
///
/// Keep stories in a separate directory as `*.story.json` files. Each contains a
/// name, a view path relative to `asset_root`, and its initial data:
///
/// ```json
/// {"name": "Low health", "view": "ui/hud.kui", "data": {"health": 12, "maximum": 100}}
/// ```
///
/// ```no_run
/// let mut app = inslag_kui_workbench::workbench_app_with_stories("assets", "stories");
/// app.run();
/// ```
pub fn workbench_app_with_stories(
    asset_root: impl Into<PathBuf>,
    story_root: impl Into<PathBuf>,
) -> App {
    configured_app(asset_root.into(), Some(story_root.into()))
}

fn configured_app(root: PathBuf, story_root: Option<PathBuf>) -> App {
    let mut app = App::new();
    let memory = Dir::default();
    memory.insert_asset_text(
        Path::new("__kui_workbench/chrome.css"),
        include_str!("chrome.css"),
    );
    let reader_root = root.clone();
    let reader_memory = memory.clone();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::platform_default(&root.to_string_lossy(), None).with_reader(
            move || {
                Box::new(fixtures::SourceReader {
                    memory: MemoryAssetReader {
                        root: reader_memory.clone(),
                    },
                    files: FileAssetReader::new(reader_root.clone()),
                })
            },
        ),
    );
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: root.to_string_lossy().into_owned(),
                watch_for_changes_override: Some(true),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "KUI Workbench".into(),
                    resolution: (1280, 800).into(),
                    ..default()
                }),
                ..default()
            }),
    );
    let mut workbench = WorkbenchPlugin::new(root);
    workbench.story_root = story_root;
    workbench.memory = memory;
    app.add_plugins((
        FlairPlugin,
        StateTreePlugin::new(Update),
        KuiStateTreePlugin,
        workbench,
    ));
    #[cfg(feature = "aseprite")]
    app.add_plugins(inslag_kui::aseprite::KuiAsepritePlugin);
    app
}

struct WorkbenchPlugin {
    asset_root: PathBuf,
    story_root: Option<PathBuf>,
    memory: Dir,
}

impl WorkbenchPlugin {
    fn new(asset_root: impl Into<PathBuf>) -> Self {
        Self {
            asset_root: asset_root.into(),
            story_root: None,
            memory: Dir::default(),
        }
    }
}

impl Plugin for WorkbenchPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((fixtures::FixturePlugin, EnhancedInputPlugin))
            .add_input_context::<editor::Editor>();
        let mut tool = Workbench::new(self.asset_root.clone());
        tool.story_root = self.story_root.clone();
        tool.memory = self.memory.clone();
        app.insert_resource(tool)
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (apply_actions, update)
                    .chain()
                    .in_set(StateTreeSet::HandleEffects),
            )
            .add_systems(
                Update,
                preview::attach.after(StateTreeSet::HandleActivities),
            )
            .add_systems(
                PostUpdate,
                preview::layout.after(bevy::ui::UiSystems::Layout),
            )
            .add_systems(
                First,
                preview::pointer.in_set(bevy::picking::PickingSystems::PostInput),
            )
            .add_systems(
                PostUpdate,
                editor::finish.after(bevy::text::EditableTextSystems),
            )
            .add_observer(editor::accept)
            .add_observer(editor::commit)
            .add_observer(editor::close)
            .add_observer(editor::changed)
            .add_observer(scroll)
            .add_observer(resize_panes);
    }
}

#[derive(Message, Clone)]
struct ToolAction {
    view: Entity,
    action: String,
    value: Option<String>,
    position: KuiValue,
    start: KuiValue,
}

#[derive(Default)]
struct Session {
    data: Data,
    initialized: bool,
}

#[derive(Resource)]
struct Workbench {
    root: PathBuf,
    story_root: Option<PathBuf>,
    stories: BTreeMap<String, stories::Story>,
    selected_story: Option<String>,
    story_error: String,
    paths: Vec<String>,
    selected: Option<String>,
    sessions: BTreeMap<String, Session>,
    bindings: Vec<Binding>,
    definition: Option<kui::KuiDefinition>,
    preview_mount: Option<Entity>,
    chrome_mount: Option<Entity>,
    chrome_asset: String,
    memory: Dir,
    chrome_revision: u64,
    preview_data: Option<Data>,
    chrome_data: Option<Data>,
    chrome_camera: Option<Entity>,
    preview_camera: Option<Entity>,
    image: Handle<Image>,
    rebuild: bool,
    refresh_source: bool,
    raw: bool,
    filter: String,
    edit: Option<Entity>,
    clipboard: Option<arboard::Clipboard>,
    error: String,
    discovery_error: String,
    log: Vec<String>,
    next_action: u64,
    last_scan: Duration,
    viewport: Option<UVec2>,
    fit: bool,
    background: usize,
    left_width: f32,
    right_width: f32,
    show_views: bool,
    show_inspector: bool,
    preview_rect: Rect,
    preview_clip_rect: Rect,
    resolution: UVec2,
}

impl Workbench {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            story_root: None,
            stories: BTreeMap::new(),
            selected_story: None,
            story_error: String::new(),
            paths: vec![],
            selected: None,
            sessions: BTreeMap::new(),
            bindings: vec![],
            definition: None,
            preview_mount: None,
            chrome_mount: None,
            chrome_asset: String::new(),
            memory: Dir::default(),
            chrome_revision: 0,
            preview_data: None,
            chrome_data: None,
            chrome_camera: None,
            preview_camera: None,
            image: Handle::default(),
            rebuild: true,
            refresh_source: true,
            raw: false,
            filter: String::new(),
            edit: None,
            clipboard: None,
            error: String::new(),
            discovery_error: String::new(),
            log: vec![],
            next_action: 1,
            last_scan: Duration::ZERO,
            viewport: None,
            fit: true,
            background: 0,
            left_width: 180.,
            right_width: 340.,
            show_views: true,
            show_inspector: true,
            preview_rect: Rect::default(),
            preview_clip_rect: Rect::default(),
            resolution: UVec2::new(800, 600),
        }
    }

    fn data(&self) -> Option<&Data> {
        self.session_key()
            .and_then(|path| self.sessions.get(&path))
            .map(|session| &session.data)
    }

    fn data_mut(&mut self) -> Option<&mut Data> {
        self.session_key()
            .and_then(|path| self.sessions.get_mut(&path))
            .map(|session| &mut session.data)
    }

    fn session_key(&self) -> Option<String> {
        self.selected
            .as_ref()
            .map(|path| match &self.selected_story {
                Some(story) => format!("{path}\0{story}"),
                None => path.clone(),
            })
    }

    fn initial_session(&self) -> Session {
        self.selected_story
            .as_ref()
            .and_then(|id| self.stories.get(id))
            .map(|story| Session {
                data: story.data.clone(),
                initialized: true,
            })
            .unwrap_or_default()
    }
}

fn setup(world: &mut World) {
    world.resource_scope(|world, mut tool: Mut<Workbench>| {
        tool.chrome_camera = Some(world.spawn(Camera2d).id());
        preview::setup(world, &mut tool);
        scan(&mut tool);
        if let Some(path) = tool.paths.first().cloned() {
            select(world, &mut tool, path);
        }
    });
}

fn apply_actions(world: &mut World) {
    let actions: Vec<_> = world
        .resource_mut::<Messages<ToolAction>>()
        .drain()
        .collect();
    world.resource_scope(|world, mut tool: Mut<Workbench>| {
        for ToolAction {
            view: mount,
            action,
            value,
            position,
            start,
            ..
        } in actions
        {
            if Some(mount) == tool.preview_mount {
                editor::end(world, &mut tool);
                let elapsed = world.resource::<Time>().elapsed_secs();
                let line = format!(
                    "#{}  {elapsed:.2}s  {}",
                    tool.next_action,
                    serde_json::json!({
                        "view": tool.selected, "action": action, "value": value,
                        "position": data::to_json(&position),
                        "start": data::to_json(&start),
                    })
                );
                tool.log.push(line);
                tool.next_action += 1;
                if tool.log.len() > 200 {
                    tool.log.remove(0);
                }
                continue;
            }
            if Some(mount) != tool.chrome_mount {
                continue;
            }
            let value = value.unwrap_or_default();
            match action.as_str() {
                "select" => select(world, &mut tool, value),
                "story" => {
                    if let Some(story) = tool.stories.get(&value) {
                        let path = story.view.clone();
                        tool.selected_story = Some(value);
                        select(world, &mut tool, path);
                    }
                }
                "filter" => {
                    let text = tool.filter.clone();
                    editor::begin(world, &mut tool, editor::Target::Filter, text);
                }
                "edit" => {
                    let current = tool.data().and_then(|data| data::get(data, &value));
                    let text = match current {
                        Some(value) => data::to_json(value).to_string(),
                        None => String::new(),
                    };
                    editor::begin(world, &mut tool, editor::Target::Binding(value), text);
                    tool.error.clear();
                }
                "toggle" => {
                    let current = tool.data().and_then(|data| data::get(data, &value));
                    let next = !matches!(current, Some(KuiValue::Boolean(true)));
                    if let Some(data) = tool.data_mut() {
                        tool.error = data::set(data, &value, Some(KuiValue::Boolean(next)))
                            .err()
                            .unwrap_or_default();
                    }
                    editor::end(world, &mut tool);
                }
                "unset" | "nil" => {
                    if let Some(data) = tool.data_mut() {
                        tool.error =
                            data::set(data, &value, (action == "nil").then_some(KuiValue::Nil))
                                .err()
                                .unwrap_or_default();
                    }
                    editor::end(world, &mut tool);
                }
                "mode" => {
                    tool.raw = !tool.raw;
                    editor::end(world, &mut tool);
                    tool.rebuild = true;
                }
                "raw" => {
                    let text = tool.data().map(data::encode).unwrap_or_else(|| "{}".into());
                    editor::begin(world, &mut tool, editor::Target::Raw, text);
                    tool.error.clear();
                }
                "copy" | "copy_log" => {
                    let text = if action == "copy_log" {
                        tool.log.join("\n")
                    } else {
                        tool.data().map(data::encode).unwrap_or_else(|| "{}".into())
                    };
                    tool.error = editor::clipboard_write(&mut tool, text)
                        .err()
                        .unwrap_or_default();
                }
                "paste" => {
                    match editor::clipboard_read(&mut tool).and_then(|text| data::decode(&text)) {
                        Ok(data) => {
                            if let Some(current) = tool.data_mut() {
                                *current = data;
                            }
                            editor::end(world, &mut tool);
                            tool.error.clear();
                        }
                        Err(error) => tool.error = error,
                    }
                }
                "reset" => {
                    if let Some(path) = tool.selected.clone() {
                        let initial = tool.initial_session();
                        let key = tool.session_key().unwrap();
                        tool.sessions.insert(key, initial);
                        select(world, &mut tool, path);
                    }
                }
                "clear_log" => tool.log.clear(),
                "rescan" => {
                    scan(&mut tool);
                    tool.rebuild = true;
                }
                "viewport" => {
                    tool.viewport = match value.as_str() {
                        "720" => Some(UVec2::new(1280, 720)),
                        "1080" => Some(UVec2::new(1920, 1080)),
                        "mobile" => Some(UVec2::new(390, 844)),
                        "custom" => {
                            let text = format!("{}x{}", tool.resolution.x, tool.resolution.y);
                            editor::begin(world, &mut tool, editor::Target::Viewport, text);
                            tool.viewport
                        }
                        _ => None,
                    };
                }
                "zoom" => tool.fit = !tool.fit,
                "background" => tool.background = (tool.background + 1) % 3,
                "views" => tool.show_views = !tool.show_views,
                "inspector" => tool.show_inspector = !tool.show_inspector,
                _ => {}
            }
        }
    });
}

fn update(world: &mut World) {
    world.resource_scope(|world, mut tool: Mut<Workbench>| {
        let elapsed = world.resource::<Time>().elapsed();
        if elapsed.saturating_sub(tool.last_scan) > Duration::from_secs(2) {
            scan(&mut tool);
            tool.last_scan = elapsed;
        }
        if tool
            .selected_story
            .as_ref()
            .is_some_and(|id| !tool.stories.contains_key(id))
            && let Some(path) = tool.selected.clone()
        {
            tool.selected_story = None;
            select(world, &mut tool, path);
        }
        if tool.selected.is_none()
            && let Some(path) = tool.paths.first().cloned()
        {
            select(world, &mut tool, path);
        }
        let definition = tool
            .selected
            .clone()
            .filter(|_| tool.refresh_source)
            .as_ref()
            .and_then(|path| match data::load(&tool.root, path) {
                Ok(definition) => Some(definition),
                Err(error) => {
                    tool.error = error;
                    None
                }
            });
        tool.refresh_source = false;
        if let Some(definition) = definition
            && tool.definition.as_ref() != Some(&definition)
        {
            refresh_definition(&mut tool, definition);
        }
        if tool.rebuild {
            rebuild_chrome(world, &mut tool);
            tool.rebuild = false;
        }
        if let Some(mount) = tool.preview_mount {
            let data = tool.data().cloned().unwrap_or_default();
            if tool.preview_data.as_ref() != Some(&data) {
                let roots_changed = tool
                    .preview_data
                    .as_ref()
                    .is_none_or(|old| !old.keys().eq(data.keys()));
                if roots_changed {
                    if let Err(error) = fixtures::install(
                        world,
                        mount,
                        tool.selected.as_deref().expect("selected preview"),
                        &data,
                    ) {
                        tool.error = error;
                    }
                } else {
                    fixtures::update(world, mount, data.clone());
                }
                tool.preview_data = Some(data);
            }
        }
        sync_chrome(world, &mut tool);
    });
}

fn scan(tool: &mut Workbench) {
    tool.refresh_source = true;
    if let Some(root) = &tool.story_root {
        match stories::discover(root) {
            Ok(stories) => {
                if stories != tool.stories {
                    for (id, story) in &stories {
                        if let Some(old) = tool.stories.get(id) {
                            let key = format!("{}\0{id}", old.view);
                            if let Some(session) = tool.sessions.get_mut(&key)
                                && session.data == old.data
                                && story.view == old.view
                            {
                                session.data = story.data.clone();
                            }
                        }
                    }
                    tool.stories = stories;
                    tool.rebuild = true;
                }
                tool.story_error.clear();
            }
            Err(error) => tool.story_error = error,
        }
    }
    match discover(&tool.root) {
        Ok(paths) => {
            if paths != tool.paths {
                tool.paths = paths;
                tool.rebuild = true;
            }
            tool.discovery_error.clear();
        }
        Err(error) => tool.discovery_error = error,
    }
}

fn select(world: &mut World, tool: &mut Workbench, path: String) {
    if let Some(mount) = tool.preview_mount.take() {
        world.despawn(mount);
    }
    if tool
        .selected_story
        .as_ref()
        .and_then(|id| tool.stories.get(id))
        .is_none_or(|story| story.view != path)
    {
        tool.selected_story = tool
            .stories
            .iter()
            .find(|(_, story)| story.view == path)
            .map(|(id, _)| id.clone());
    }
    tool.selected = Some(path.clone());
    let initial = tool.initial_session();
    tool.sessions
        .entry(tool.session_key().unwrap())
        .or_insert(initial);
    tool.definition = None;
    tool.refresh_source = true;
    tool.bindings.clear();
    editor::end(world, tool);
    tool.error.clear();
    let controller = world
        .spawn((
            Name::new("Workbench preview controller"),
            UiTargetCamera(tool.preview_camera.expect("preview camera")),
        ))
        .id();
    tool.preview_mount = Some(controller);
    tool.preview_data = None;
    tool.rebuild = true;
}

/// Deterministic filesystem discovery. Symlink directories are not followed.
fn discover(root: &Path) -> Result<Vec<String>, String> {
    fn visit(root: &Path, dir: &Path, result: &mut Vec<String>) -> Result<(), String> {
        for entry in
            std::fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))?
        {
            let entry = entry.map_err(|error| error.to_string())?;
            let ty = entry.file_type().map_err(|error| error.to_string())?;
            if ty.is_dir() {
                visit(root, &entry.path(), result)?;
            } else if ty.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "kui")
            {
                let path = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                result.push(path);
            }
        }
        Ok(())
    }
    let mut paths = vec![];
    visit(root, root, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn refresh_definition(tool: &mut Workbench, definition: kui::KuiDefinition) {
    tool.bindings = data::bindings(&definition);
    let path = tool.session_key().expect("selected view has a definition");
    let session = tool
        .sessions
        .get_mut(&path)
        .expect("selected view has a session");
    if !session.initialized {
        for binding in &tool.bindings {
            if binding.kind != Kind::Conflict {
                let _ = data::set(
                    &mut session.data,
                    &binding.path,
                    Some(binding.sample.clone()),
                );
            }
        }
        session.initialized = true;
    }
    tool.definition = Some(definition);
    tool.rebuild = true;
}

fn rebuild_chrome(world: &mut World, tool: &mut Workbench) {
    let mut views = String::new();
    for (index, path) in tool
        .paths
        .iter()
        .enumerate()
        .filter(|(_, path)| path.to_lowercase().contains(&tool.filter.to_lowercase()))
    {
        let label = Path::new(path)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        views.push_str(&format!("<button on-click=\"select\" value=\"{}\" bind:attr-selected=\"selected_{index}\"><text>{}</text></button>", xml(path), xml(&label)));
    }
    let mut fields = String::new();
    let mut story_buttons = String::new();
    for (id, story) in &tool.stories {
        if Some(&story.view) == tool.selected.as_ref() {
            story_buttons.push_str(&format!(
                "<button on-click=\"story\" value=\"{}\" class=\"{}\"><text>{}</text></button>",
                xml(id),
                if tool.selected_story.as_ref() == Some(id) {
                    "selected-story"
                } else {
                    ""
                },
                xml(&story.name)
            ));
        }
    }
    for (index, binding) in tool.bindings.iter().enumerate() {
        fields.push_str(&format!("<node class=\"binding\"><text class=\"label\">{}</text><node class=\"row\"><button class=\"field\" on-click=\"{}\" value=\"{}\"><text bind:text=\"field_{index}\"/></button>{}{}</node><text class=\"muted\">{}</text></node>",
            xml(&binding.path), if binding.kind == Kind::Boolean { "toggle" } else { "edit" }, xml(&binding.path),
            button("unset", &binding.path, "Unset"), button("nil", &binding.path, "Nil"), xml(&binding.uses.join(", "))));
    }
    let source = include_str!("chrome.kui")
        .replace("<node class=\"workbench\">", "<style src=\"__kui_workbench/chrome.css\"/><node class=\"workbench\">")
        .replace("<node id=\"views-list\" class=\"list\"/>", &format!("<node id=\"views-list\" class=\"list\">{views}</node>"))
        .replace("<node id=\"stories-list\" class=\"list\"/>", &format!("<node id=\"stories-list\" class=\"list\">{story_buttons}</node>"))
        .replace("<node id=\"bindings-list\" class=\"bindings\" bind:visible=\"controls_mode\"/>", &format!("<node id=\"bindings-list\" class=\"bindings\" bind:visible=\"controls_mode\">{fields}</node>"));
    if let Some(mount) = tool.chrome_mount.take() {
        world.despawn(mount);
    }
    if !tool.chrome_asset.is_empty() {
        tool.memory.remove_asset(Path::new(&tool.chrome_asset));
    }
    tool.chrome_revision += 1;
    tool.chrome_asset = format!("__kui_workbench/chrome-{}.kui", tool.chrome_revision);
    tool.memory
        .insert_asset_text(Path::new(&tool.chrome_asset), &source);
    tool.chrome_mount = Some(
        world
            .spawn((
                Name::new("Workbench chrome controller"),
                UiTargetCamera(tool.chrome_camera.expect("chrome camera")),
            ))
            .id(),
    );
    tool.chrome_data = None;
}

fn button(action: &str, value: &str, label: &str) -> String {
    format!(
        "<button on-click=\"{}\" value=\"{}\"><text>{}</text></button>",
        xml(action),
        xml(value),
        xml(label)
    )
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn sync_chrome(world: &mut World, tool: &mut Workbench) {
    let mut values = Data::new();
    let mut text = |key: &str, value: String| {
        values.insert(key.into(), KuiValue::String(value));
    };
    text(
        "title",
        match (&tool.selected, &tool.definition) {
            (Some(path), Some(definition)) => format!("{} · {path}", definition.name),
            (Some(path), None) => path.clone(),
            _ => "No .kui files found".into(),
        },
    );
    text(
        "viewport",
        format!(
            "{}x{} {}",
            tool.resolution.x,
            tool.resolution.y,
            if tool.viewport.is_none() {
                "auto"
            } else if tool.fit {
                "fit"
            } else {
                "1:1"
            }
        ),
    );
    text("filter", format!("Filter: {}", tool.filter));
    text(
        "raw",
        tool.data().map(data::encode).unwrap_or_else(|| "{}".into()),
    );
    text(
        "mode_label",
        if tool.raw { "Controls" } else { "Raw JSON" }.into(),
    );
    let diagnostics = tool
        .bindings
        .iter()
        .filter_map(|binding| {
            let value = tool.data().and_then(|data| data::get(data, &binding.path));
            let valid = match (&binding.kind, value) {
                (Kind::Boolean, Some(KuiValue::Boolean(_)))
                | (Kind::Sequence, Some(KuiValue::Sequence(_))) => true,
                (Kind::Record, Some(KuiValue::Record(_))) => true,
                (Kind::Lookup, Some(KuiValue::Record(_) | KuiValue::Sequence(_))) => true,
                (Kind::String, Some(KuiValue::String(_)))
                | (Kind::Integer, Some(KuiValue::Integer(_))) => true,
                (
                    Kind::Text,
                    Some(KuiValue::String(_) | KuiValue::Boolean(_) | KuiValue::Integer(_)),
                ) => true,
                (Kind::Number, Some(KuiValue::Integer(_))) => true,
                (
                    Kind::Scalar,
                    Some(
                        KuiValue::Nil
                        | KuiValue::Boolean(_)
                        | KuiValue::String(_)
                        | KuiValue::Integer(_),
                    ),
                ) => true,
                (Kind::Scalar | Kind::Text | Kind::Number, Some(KuiValue::Number(number))) => {
                    number.is_finite()
                }
                _ => false,
            };
            if valid {
                None
            } else {
                Some(format!(
                    "{}: {}",
                    binding.path,
                    if value.is_none() {
                        "missing"
                    } else {
                        "incompatible binding type"
                    }
                ))
            }
        })
        .collect::<Vec<_>>();
    text(
        "error",
        if tool.error.is_empty() {
            diagnostics
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            tool.error.clone()
        },
    );
    text(
        "log",
        tool.log
            .iter()
            .rev()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let status = tool
        .preview_mount
        .and_then(|controller| world.get::<StateTreeInstance>(controller))
        .and_then(|tree| tree.data("_workbench_render_status"))
        .map(|status| format!("{status:?}"))
        .unwrap_or_else(|| "No view selected".into());
    text(
        "status",
        if !tool.story_error.is_empty() {
            tool.story_error.clone()
        } else if !tool.discovery_error.is_empty() {
            tool.discovery_error.clone()
        } else {
            format!(
                "{status} · {} views · {}",
                tool.paths.len(),
                tool.selected_story
                    .as_ref()
                    .and_then(|id| tool.stories.get(id))
                    .map(|story| story.name.as_str())
                    .unwrap_or("Binding defaults")
            )
        },
    );
    for (index, binding) in tool.bindings.iter().enumerate() {
        let value = tool.data().and_then(|data| data::get(data, &binding.path));
        let display = match value {
            Some(KuiValue::Sequence(items)) if binding.kind == Kind::Sequence => {
                format!("[{} items] · Edit JSON", items.len())
            }
            Some(KuiValue::Boolean(value)) => format!("[{}]", if *value { "x" } else { " " }),
            Some(KuiValue::String(value)) => value.clone(),
            Some(value) => data::to_json(value).to_string(),
            None => "<unset>".into(),
        };
        text(&format!("field_{index}"), display);
    }
    values.insert("show_views".into(), KuiValue::Boolean(tool.show_views));
    values.insert("raw_mode".into(), KuiValue::Boolean(tool.raw));
    values.insert("controls_mode".into(), KuiValue::Boolean(!tool.raw));
    values.insert(
        "show_inspector".into(),
        KuiValue::Boolean(tool.show_inspector),
    );
    for (index, path) in tool.paths.iter().enumerate() {
        values.insert(
            format!("selected_{index}"),
            KuiValue::Boolean(tool.selected.as_ref() == Some(path)),
        );
    }
    if let Some(controller) = tool.chrome_mount
        && tool.chrome_data.as_ref() != Some(&values)
    {
        if tool
            .chrome_data
            .as_ref()
            .is_none_or(|old| !old.keys().eq(values.keys()))
        {
            fixtures::install(world, controller, &tool.chrome_asset, &values)
                .expect("valid authored chrome controller");
        } else {
            fixtures::update(world, controller, values.clone());
        }
        tool.chrome_data = Some(values);
    }
}

fn scroll(
    mut event: On<PointerScroll>,
    mut query: Query<(&ComputedNode, &mut ScrollPosition, &Node)>,
) {
    if let Ok((computed, mut position, node)) = query.get_mut(event.entity) {
        let delta = if event.unit == bevy::input::mouse::MouseScrollUnit::Line {
            event.y * 24.
        } else {
            event.y
        };
        if node.overflow.y == OverflowAxis::Scroll {
            let maximum =
                (computed.content_size.y - computed.size.y).max(0.) * computed.inverse_scale_factor;
            position.0.y = (position.0.y - delta).clamp(0., maximum);
            event.propagate(false);
        }
    }
}

fn resize_panes(
    event: On<PointerDrag>,
    names: Query<&Name, With<preview::ChromeNode>>,
    mut tool: ResMut<Workbench>,
) {
    let Ok(name) = names.get(event.entity) else {
        return;
    };
    match name.as_str() {
        "left-splitter" => tool.left_width = (tool.left_width + event.delta.x).clamp(90., 500.),
        "right-splitter" => tool.right_width = (tool.right_width - event.delta.x).clamp(180., 700.),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inslag_kui::source as inslag_kui;

    #[test]
    fn story_sessions_are_independent_and_file_refresh_preserves_edits() {
        let root = tempfile::tempdir().unwrap();
        let story_file = root.path().join("one.story.json");
        std::fs::write(
            &story_file,
            r#"{"name":"One","view":"one.kui","data":{"title":"Authored"}}"#,
        )
        .unwrap();
        let mut tool = Workbench::new(root.path().into());
        tool.story_root = Some(root.path().into());
        scan(&mut tool);
        tool.selected = Some("one.kui".into());
        tool.selected_story = Some("one.story.json".into());
        tool.sessions
            .insert(tool.session_key().unwrap(), tool.initial_session());
        let original_key = tool.session_key().unwrap();
        std::fs::write(
            &story_file,
            r#"{"name":"One","view":"one.kui","data":{"title":"Reloaded"}}"#,
        )
        .unwrap();
        scan(&mut tool);
        assert_eq!(
            data::get(tool.data().unwrap(), "title"),
            Some(&KuiValue::String("Reloaded".into()))
        );
        data::set(
            tool.data_mut().unwrap(),
            "title",
            Some(KuiValue::String("Edited".into())),
        )
        .unwrap();
        std::fs::write(
            &story_file,
            r#"{"name":"One","view":"one.kui","data":{"title":"Changed again"}}"#,
        )
        .unwrap();
        scan(&mut tool);
        assert_eq!(
            data::get(tool.data().unwrap(), "title"),
            Some(&KuiValue::String("Edited".into()))
        );
        tool.selected_story = None;
        let other_key = tool.session_key().unwrap();
        assert_ne!(original_key, other_key);
        tool.sessions.insert(other_key, Session::default());
        assert!(tool.data().unwrap().is_empty());
        tool.selected_story = Some("one.story.json".into());
        let reset = tool.initial_session();
        tool.sessions.insert(tool.session_key().unwrap(), reset);
        assert_eq!(
            data::get(tool.data().unwrap(), "title"),
            Some(&KuiValue::String("Changed again".into()))
        );
        std::fs::write(&story_file, "incomplete {").unwrap();
        scan(&mut tool);
        assert!(!tool.story_error.is_empty());
        assert_eq!(
            tool.initial_session().data["title"],
            KuiValue::String("Changed again".into())
        );
    }

    #[test]
    fn domain_commands_log_payloads_without_executing_application_actions() {
        let mut app = App::new();
        app.init_resource::<Time>().add_message::<ToolAction>();
        let owner = app.world_mut().spawn_empty().id();
        let mount = owner;
        let mut tool = Workbench::new("unused".into());
        tool.preview_mount = Some(mount);
        tool.selected = Some("fixture.kui".into());
        app.insert_resource(tool);
        for value in [None, Some(String::new()), Some("oak".into())] {
            app.world_mut().write_message(ToolAction {
                view: mount,
                action: "quit".into(),
                value,
                position: KuiValue::Nil,
                start: KuiValue::Nil,
            });
        }
        apply_actions(app.world_mut());
        let tool = app.world().resource::<Workbench>();
        assert_eq!(tool.log.len(), 3);
        assert!(tool.log[0].contains("\"value\":null"));
        assert!(tool.log[1].contains("\"value\":\"\""));
        assert!(tool.log[2].contains("\"value\":\"oak\""));
        assert!(tool.log[0].contains("\"action\":\"quit\""));
        assert!(app.world().get_entity(mount).is_ok());
        assert_eq!(tool.next_action, 4);
    }

    #[test]
    fn hot_reload_rediscovers_bindings_without_restoring_unset_or_edited_data() {
        let parse = |children: &str| {
            inslag_kui::parse("synthetic.kui", &format!(r#"<view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="test"><node>{children}</node></view>"#)).unwrap()
        };
        let mut tool = Workbench::new("unused".into());
        tool.selected = Some("fixture.kui".into());
        tool.sessions
            .insert("fixture.kui".into(), Session::default());
        refresh_definition(
            &mut tool,
            parse(r#"<text bind:text="title">Fallback</text><node bind:visible="open"/>"#),
        );
        assert_eq!(
            data::get(tool.data().unwrap(), "title"),
            Some(&KuiValue::String("Fallback".into()))
        );
        data::set(
            tool.data_mut().unwrap(),
            "title",
            Some(KuiValue::String("Edited".into())),
        )
        .unwrap();
        data::set(tool.data_mut().unwrap(), "open", None).unwrap();
        refresh_definition(
            &mut tool,
            parse(
                r#"<text bind:text="title">Changed fallback</text><node bind:visible="open"/><text bind:text="new">New</text>"#,
            ),
        );
        assert_eq!(
            data::get(tool.data().unwrap(), "title"),
            Some(&KuiValue::String("Edited".into()))
        );
        assert_eq!(data::get(tool.data().unwrap(), "open"), None);
        assert_eq!(data::get(tool.data().unwrap(), "new"), None);
        assert!(tool.bindings.iter().any(|binding| binding.path == "new"));
    }

    #[test]
    fn discovery_is_recursive_sorted_and_only_includes_kui() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("nested")).unwrap();
        std::fs::write(root.path().join("z.kui"), "").unwrap();
        std::fs::write(root.path().join("nested/a.kui"), "").unwrap();
        std::fs::write(root.path().join("not.css"), "").unwrap();
        assert_eq!(discover(root.path()).unwrap(), ["nested/a.kui", "z.kui"]);
        assert!(discover(&root.path().join("missing")).is_err());
    }

    #[test]
    fn authored_strings_are_escaped_in_generated_controls() {
        let source = format!(
            r#"<view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="test"><node>{}</node></view>"#,
            button("select", "a&b.kui", "<hello>")
        );
        let definition = inslag_kui::parse("test", &source).unwrap();
        let inslag_kui::KuiNodeKind::Node { children } = &definition.children[0].kind else {
            panic!()
        };
        assert_eq!(children[0].attributes["value"], "a&b.kui");
        let inslag_kui::KuiNodeKind::Button { children } = &children[0].kind else {
            panic!()
        };
        assert_eq!(
            children[0].kind,
            inslag_kui::KuiNodeKind::Text {
                value: "<hello>".into()
            }
        );
    }
}
