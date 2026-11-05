use bevy::{
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, TextEdit, TextEditChange},
    ui_widgets::TextInput,
};
use bevy_enhanced_input::prelude::*;
use bevy_flair::prelude::{ClassList, Styled, TypeName};

use crate::{Workbench, data};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Binding(String),
    Raw,
    Filter,
    Viewport,
}

#[derive(Component)]
pub(crate) struct Editor(Target);

#[derive(Component)]
pub(crate) struct EndEditing(bool);

#[derive(InputAction)]
#[action_output(bool)]
pub(crate) struct AcceptEdit;

#[derive(InputAction)]
#[action_output(bool)]
pub(crate) struct CommitEdit;

#[derive(InputAction)]
#[action_output(bool)]
pub(crate) struct CloseEdit;

pub(crate) fn begin(world: &mut World, tool: &mut Workbench, target: Target, value: String) {
    end(world, tool);
    let mut text = EditableText::new(value);
    text.allow_newlines = target == Target::Raw;
    text.visible_lines = Some(if text.allow_newlines { 8. } else { 1. });
    text.queue_edit(TextEdit::SelectAll);
    let style = world
        .resource::<AssetServer>()
        .load("__kui_workbench/chrome.css");
    let mut entity = world.spawn((
        Editor(target),
        text,
        TextInput,
        Styled::new(style),
        ClassList::new("native-editor"),
        TypeName("text-input"),
        GlobalZIndex(100),
    ));
    if let Some(camera) = tool.chrome_camera {
        entity.insert(UiTargetCamera(camera));
    }
    let entity = entity.id();
    let commit = world
        .spawn((Action::<CommitEdit>::new(), ActionOf::<Editor>::new(entity)))
        .id();
    for modifier in [ModKeys::CONTROL, ModKeys::SUPER] {
        world.spawn((
            Binding::Keyboard {
                key: KeyCode::Enter,
                mod_keys: modifier,
            },
            BindingOf(commit),
        ));
    }
    let accept = world
        .spawn((Action::<AcceptEdit>::new(), ActionOf::<Editor>::new(entity)))
        .id();
    world.spawn((Binding::from(KeyCode::Enter), BindingOf(accept)));
    let close = world
        .spawn((Action::<CloseEdit>::new(), ActionOf::<Editor>::new(entity)))
        .id();
    world.spawn((Binding::from(KeyCode::Escape), BindingOf(close)));
    world
        .resource_mut::<InputFocus>()
        .set(entity, FocusCause::Auto);
    tool.edit = Some(entity);
}

pub(crate) fn accept(
    event: On<Start<AcceptEdit>>,
    editors: Query<(&Editor, &EditableText)>,
    mut commands: Commands,
) {
    if let Ok((editor, text)) = editors.get(event.context)
        && editor.0 != Target::Raw
        && !text.is_composing()
    {
        commands.entity(event.context).insert(EndEditing(true));
    }
}

pub(crate) fn commit(
    event: On<Start<CommitEdit>>,
    editors: Query<&EditableText, With<Editor>>,
    mut commands: Commands,
) {
    if let Ok(text) = editors.get(event.context)
        && !text.is_composing()
    {
        commands.entity(event.context).insert(EndEditing(true));
    }
}

pub(crate) fn close(
    event: On<Start<CloseEdit>>,
    editors: Query<&EditableText, With<Editor>>,
    mut commands: Commands,
) {
    if let Ok(text) = editors.get(event.context)
        && !text.is_composing()
    {
        commands.entity(event.context).insert(EndEditing(false));
    }
}

pub(crate) fn changed(
    event: On<TextEditChange>,
    editors: Query<(&Editor, &EditableText)>,
    mut tool: ResMut<Workbench>,
) {
    if tool.edit != Some(event.event_target()) {
        return;
    }
    if let Ok((editor, text)) = editors.get(event.event_target()) {
        tool.error = apply(&editor.0, &text.value().to_string(), &mut tool)
            .err()
            .unwrap_or_default();
    }
}

pub(crate) fn finish(world: &mut World) {
    world.resource_scope(|world, mut tool: Mut<Workbench>| {
        let Some(entity) = tool.edit else { return };
        let Some(ending) = world.get::<EndEditing>(entity) else {
            return;
        };
        if ending.0 {
            let editor = world.get::<Editor>(entity).unwrap();
            let text = world.get::<EditableText>(entity).unwrap();
            if let Err(error) = apply(&editor.0, &text.value().to_string(), &mut tool) {
                tool.error = error;
                world.entity_mut(entity).remove::<EndEditing>();
                return;
            }
        }
        end(world, &mut tool);
    });
}

fn apply(target: &Target, text: &str, tool: &mut Workbench) -> Result<(), String> {
    match target {
        Target::Binding(path) => {
            let value =
                data::from_json(serde_json::from_str(text).map_err(|error| error.to_string())?)?;
            if let Some(data) = tool.data_mut() {
                data::set(data, path, Some(value))?;
            }
        }
        Target::Raw => {
            let data = data::decode(text)?;
            if let Some(current) = tool.data_mut() {
                *current = data;
            }
        }
        Target::Filter => {
            if tool.filter != text {
                tool.filter = text.into();
                tool.rebuild = true;
            }
        }
        Target::Viewport => {
            let (width, height) = text
                .split_once(['x', '×'])
                .ok_or("Use WIDTHxHEIGHT, e.g. 1280x720")?;
            let width = width
                .trim()
                .parse::<u32>()
                .map_err(|error| error.to_string())?;
            let height = height
                .trim()
                .parse::<u32>()
                .map_err(|error| error.to_string())?;
            if !(32..=4096).contains(&width) || !(32..=4096).contains(&height) {
                return Err("Viewport dimensions must be between 32 and 4096".into());
            }
            tool.viewport = Some(UVec2::new(width, height));
        }
    }
    Ok(())
}

pub(crate) fn end(world: &mut World, tool: &mut Workbench) {
    if let Some(entity) = tool.edit.take() {
        if world.resource::<InputFocus>().get() == Some(entity) {
            world.resource_mut::<InputFocus>().clear();
        }
        world.despawn(entity);
    }
}

pub(crate) fn clipboard_write(tool: &mut Workbench, text: String) -> Result<(), String> {
    clipboard(tool)?
        .set_text(text)
        .map_err(|error| format!("Clipboard: {error}"))
}

pub(crate) fn clipboard_read(tool: &mut Workbench) -> Result<String, String> {
    clipboard(tool)?
        .get_text()
        .map_err(|error| format!("Clipboard: {error}"))
}

fn clipboard(tool: &mut Workbench) -> Result<&mut arboard::Clipboard, String> {
    if tool.clipboard.is_none() {
        tool.clipboard =
            Some(arboard::Clipboard::new().map_err(|error| format!("Clipboard: {error}"))?);
    }
    Ok(tool.clipboard.as_mut().expect("clipboard initialized"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        asset::io::{
            AssetSourceBuilder, AssetSourceId,
            memory::{Dir, MemoryAssetReader},
        },
        input::{
            ButtonState,
            keyboard::{Key, KeyboardInput},
        },
        window::{Ime, PrimaryWindow},
    };
    use inslag_state_tree::LuauValue;

    #[test]
    fn native_editor_handles_unicode_repeat_ime_and_enhanced_input_close() {
        let mut app = test_app(Target::Filter, "");
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        let entity = app.world().resource::<Workbench>().edit.unwrap();
        for (text, repeat) in [("日", false), ("🦀é", true)] {
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::KeyA,
                logical_key: Key::Character(text.into()),
                state: ButtonState::Pressed,
                text: Some(text.into()),
                repeat,
                window,
            });
            app.update();
        }
        assert_eq!(
            app.world()
                .get::<EditableText>(entity)
                .unwrap()
                .value()
                .to_string(),
            "日🦀é"
        );
        assert_eq!(app.world().resource::<Workbench>().filter, "日🦀é");
        app.world_mut().write_message(Ime::Commit {
            window,
            value: "本".into(),
        });
        app.update();
        assert_eq!(app.world().resource::<Workbench>().filter, "日🦀é本");
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: Key::Escape,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        app.update();
        assert!(app.world().resource::<Workbench>().edit.is_none());
        assert!(app.world().get_entity(entity).is_err());
        assert_eq!(app.world().resource::<Workbench>().filter, "日🦀é本");
    }

    #[test]
    fn enhanced_input_submit_preserves_multiline_raw_editing() {
        let mut app = test_app(Target::Raw, "{}");
        let entity = app.world().resource::<Workbench>().edit.unwrap();
        app.world_mut()
            .get_mut::<EditableText>(entity)
            .unwrap()
            .queue_edit(TextEdit::TextEnd(false));
        app.update();
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        app.update();
        assert_eq!(app.world().resource::<Workbench>().edit, Some(entity));
        assert_eq!(
            app.world()
                .get::<EditableText>(entity)
                .unwrap()
                .value()
                .to_string(),
            "{}\n"
        );
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Released,
            text: None,
            repeat: false,
            window,
        });
        app.update();
        for (key_code, logical_key) in [
            (KeyCode::ControlLeft, Key::Control),
            (KeyCode::Enter, Key::Enter),
        ] {
            app.world_mut().write_message(KeyboardInput {
                key_code,
                logical_key,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });
        }
        app.update();
        assert!(app.world().resource::<Workbench>().edit.is_none());
        assert!(app.world().get_entity(entity).is_err());
    }

    #[test]
    fn native_raw_editor_keeps_invalid_text_and_last_valid_data() {
        let mut app = test_app(Target::Raw, r#"{"title":"Valid"}"#);
        let entity = app.world().resource::<Workbench>().edit.unwrap();
        app.world_mut()
            .get_mut::<EditableText>(entity)
            .unwrap()
            .queue_edit(TextEdit::Insert("unfinished".into()));
        app.update();
        assert!(!app.world().resource::<Workbench>().error.is_empty());
        app.world_mut().entity_mut(entity).insert(EndEditing(true));
        app.update();
        assert_eq!(app.world().resource::<Workbench>().edit, Some(entity));
        assert_eq!(
            app.world()
                .resource::<Workbench>()
                .data()
                .unwrap()
                .get("title"),
            Some(&LuauValue::String("Valid".into()))
        );
        app.world_mut().entity_mut(entity).insert(EndEditing(false));
        app.update();
        assert!(app.world().resource::<Workbench>().edit.is_none());
    }

    #[test]
    fn invalid_raw_edits_keep_last_valid_data() {
        let mut tool = Workbench::new("unused".into());
        tool.selected = Some("test.kui".into());
        tool.sessions
            .insert("test.kui".into(), crate::Session::default());
        apply(&Target::Raw, r#"{"title":"Valid"}"#, &mut tool).unwrap();
        assert!(apply(&Target::Raw, r#"{"title":"unfinished"#, &mut tool).is_err());
        assert_eq!(
            tool.data().unwrap().get("title"),
            Some(&LuauValue::String("Valid".into()))
        );
    }

    fn test_app(target: Target, text: &str) -> App {
        let directory = Dir::default();
        directory.insert_asset_text(
            std::path::Path::new("__kui_workbench/chrome.css"),
            include_str!("chrome.css"),
        );
        let mut app = App::new();
        app.register_asset_source(
            AssetSourceId::Default,
            AssetSourceBuilder::new(move || {
                Box::new(MemoryAssetReader {
                    root: directory.clone(),
                })
            }),
        );
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::time::TimePlugin,
            AssetPlugin {
                watch_for_changes_override: Some(false),
                ..default()
            },
        ));
        app.init_asset::<bevy::shader::Shader>();
        app.add_plugins(
            DefaultPlugins
                .build()
                .disable::<bevy::app::TaskPoolPlugin>()
                .disable::<bevy::time::TimePlugin>()
                .disable::<AssetPlugin>()
                .disable::<bevy::log::LogPlugin>()
                .disable::<bevy::winit::WinitPlugin>()
                .disable::<bevy::render::RenderPlugin>()
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: bevy::window::ExitCondition::DontExit,
                    ..default()
                }),
        );
        app.add_plugins((bevy_flair::FlairPlugin, EnhancedInputPlugin))
            .add_input_context::<Editor>()
            .add_observer(accept)
            .add_observer(commit)
            .add_observer(close)
            .add_observer(changed)
            .add_systems(PostUpdate, finish.after(bevy::text::EditableTextSystems));
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let mut tool = Workbench::new("unused".into());
        tool.selected = Some("test.kui".into());
        tool.sessions
            .insert("test.kui".into(), crate::Session::default());
        app.finish();
        app.cleanup();
        begin(app.world_mut(), &mut tool, target, text.into());
        app.insert_resource(tool);
        app.update();
        app
    }
}
