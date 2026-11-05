//! Inject only events the platform backend would produce. Never synthesize
//! pointer hits, Interaction, KUI actions, or gameplay commands here.
use bevy::{
    input::{
        ButtonState,
        gamepad::{
            GamepadAxis, GamepadButton, GamepadConnection, GamepadConnectionEvent,
            RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent, RawGamepadEvent,
        },
        keyboard::{Key, KeyboardFocusLost, KeyboardInput, NativeKey},
        mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel},
    },
    prelude::*,
    window::{CursorMoved, PrimaryWindow, WindowEvent, WindowFocused},
};
use mlua::Table;
use std::collections::HashMap;

// Script device names cannot be used to target arbitrary gameplay entities.
#[derive(Resource, Default)]
struct Gamepads(HashMap<String, (Entity, bool)>);

pub(crate) fn deliver(world: &mut World, args: &Table) -> Result<(), String> {
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .map_err(|e| e.to_string())?;
    let kind: String = args.get("kind").map_err(|e| e.to_string())?;
    let state = if args
        .get::<Option<bool>>("down")
        .map_err(|e| e.to_string())?
        .unwrap_or(false)
    {
        ButtonState::Pressed
    } else {
        ButtonState::Released
    };
    match kind.as_str() {
        "key" => {
            let name: String = args.get("key").map_err(|e| e.to_string())?;
            // KeyCode's reflected enum avoids a perpetually incomplete manual key list.
            let registry = world.resource::<AppTypeRegistry>().read();
            let registration = registry
                .get(std::any::TypeId::of::<KeyCode>())
                .ok_or("KeyCode is not registered")?;
            use serde::de::DeserializeSeed;
            let reflected =
                bevy::reflect::serde::TypedReflectDeserializer::new(registration, &registry)
                    .deserialize(serde_json::Value::String(name.clone()))
                    .map_err(|e| e.to_string())?;
            let key_code = KeyCode::from_reflect(reflected.as_ref()).ok_or("invalid key code")?;
            drop(registry);
            let logical_key = match name.as_str() {
                "Escape" => Key::Escape,
                "Enter" => Key::Enter,
                "Space" => Key::Space,
                "Tab" => Key::Tab,
                "Backspace" => Key::Backspace,
                key if key.starts_with("Key") && key.len() == 4 => {
                    Key::Character(key[3..].to_lowercase().into())
                }
                _ => Key::Unidentified(NativeKey::Unidentified),
            };
            let event = KeyboardInput {
                key_code,
                logical_key,
                state,
                text: args
                    .get::<Option<String>>("text")
                    .map_err(|e| e.to_string())?
                    .map(Into::into),
                repeat: args
                    .get::<Option<bool>>("repeat")
                    .map_err(|e| e.to_string())?
                    .unwrap_or(false),
                window,
            };
            world.write_message(event.clone());
            world.write_message(WindowEvent::KeyboardInput(event));
        }
        "cursor" => {
            let x: f32 = args.get("x").map_err(|e| e.to_string())?;
            let y: f32 = args.get("y").map_err(|e| e.to_string())?;
            if !x.is_finite() || !y.is_finite() {
                return Err("cursor must be finite".into());
            }
            let position = Vec2::new(x, y);
            let mut window_ref = world.get_mut::<Window>(window).unwrap();
            let previous = window_ref.cursor_position();
            window_ref.set_cursor_position(Some(position));
            let event = CursorMoved {
                window,
                position,
                delta: previous.map(|old| position - old),
            };
            world.write_message(event.clone());
            world.write_message(WindowEvent::CursorMoved(event));
        }
        "button" => {
            let button = match args
                .get::<String>("button")
                .map_err(|e| e.to_string())?
                .as_str()
            {
                "Left" => MouseButton::Left,
                "Right" => MouseButton::Right,
                "Middle" => MouseButton::Middle,
                "Back" => MouseButton::Back,
                "Forward" => MouseButton::Forward,
                other => return Err(format!("unknown mouse button {other}")),
            };
            let event = MouseButtonInput {
                button,
                state,
                window,
            };
            world.write_message(event);
            world.write_message(WindowEvent::MouseButtonInput(event));
        }
        "wheel" => {
            let x: f32 = args.get("x").map_err(|e| e.to_string())?;
            let y: f32 = args.get("y").map_err(|e| e.to_string())?;
            if !x.is_finite() || !y.is_finite() {
                return Err("wheel must be finite".into());
            }
            let event = MouseWheel {
                unit: MouseScrollUnit::Line,
                x,
                y,
                window,
                phase: bevy::input::touch::TouchPhase::Moved,
            };
            world.write_message(event);
            world.write_message(WindowEvent::MouseWheel(event));
        }
        "focus" => {
            let focused: bool = args.get("focused").map_err(|e| e.to_string())?;
            world.get_mut::<Window>(window).unwrap().focused = focused;
            let event = WindowFocused { window, focused };
            world.write_message(event.clone());
            world.write_message(WindowEvent::WindowFocused(event));
            if !focused {
                // Winit is absent, so its single-window focus-loss notification
                // must come from the synthetic platform as well.
                world.write_message(KeyboardFocusLost);
                world.write_message(WindowEvent::KeyboardFocusLost(KeyboardFocusLost));
            }
        }
        "gamepad_connect" => {
            let id: String = args.get("gamepad").map_err(|e| e.to_string())?;
            let name = args
                .get::<Option<String>>("name")
                .map_err(|e| e.to_string())?
                .unwrap_or_else(|| id.clone());
            let vendor_id = args
                .get::<Option<u16>>("vendor_id")
                .map_err(|e| e.to_string())?;
            let product_id = args
                .get::<Option<u16>>("product_id")
                .map_err(|e| e.to_string())?;
            world.init_resource::<Gamepads>();
            let existing = world.resource::<Gamepads>().0.get(&id).copied();
            if existing.is_some_and(|(_, connected)| connected) {
                return Err(format!("gamepad {id} is already connected"));
            }
            let gamepad = existing.map_or_else(|| world.spawn_empty().id(), |(entity, _)| entity);
            world
                .resource_mut::<Gamepads>()
                .0
                .insert(id, (gamepad, true));
            let event = GamepadConnectionEvent {
                gamepad,
                connection: GamepadConnection::Connected {
                    name,
                    vendor_id,
                    product_id,
                },
            };
            world.write_message(event.clone());
            world.write_message(RawGamepadEvent::Connection(event));
        }
        "gamepad_disconnect" | "gamepad_button" | "gamepad_axis" => {
            let id: String = args.get("gamepad").map_err(|e| e.to_string())?;
            let gamepad = world
                .get_resource::<Gamepads>()
                .and_then(|pads| pads.0.get(&id))
                .filter(|(_, connected)| *connected)
                .map(|(entity, _)| *entity)
                .ok_or_else(|| format!("gamepad {id} is not connected"))?;
            if kind == "gamepad_disconnect" {
                world.resource_mut::<Gamepads>().0.get_mut(&id).unwrap().1 = false;
                let event = GamepadConnectionEvent {
                    gamepad,
                    connection: GamepadConnection::Disconnected,
                };
                world.write_message(event.clone());
                world.write_message(RawGamepadEvent::Connection(event));
            } else {
                let value: f32 = args.get("value").map_err(|e| e.to_string())?;
                let minimum = if kind == "gamepad_button" { 0.0 } else { -1.0 };
                if !value.is_finite() || !(minimum..=1.0).contains(&value) {
                    return Err(format!("gamepad value must be finite and in {minimum}..=1"));
                }
                if kind == "gamepad_button" {
                    let button: String = args.get("button").map_err(|e| e.to_string())?;
                    let button: GamepadButton =
                        serde_json::from_value(serde_json::Value::String(button))
                            .map_err(|e| e.to_string())?;
                    let event = RawGamepadButtonChangedEvent::new(gamepad, button, value);
                    world.write_message(event);
                    world.write_message(RawGamepadEvent::Button(event));
                } else {
                    let axis: String = args.get("axis").map_err(|e| e.to_string())?;
                    let axis: GamepadAxis = serde_json::from_value(serde_json::Value::String(axis))
                        .map_err(|e| e.to_string())?;
                    let event = RawGamepadAxisChangedEvent::new(gamepad, axis, value);
                    world.write_message(event);
                    world.write_message(RawGamepadEvent::Axis(event));
                }
            }
        }
        _ => return Err(format!("unknown raw input {kind}")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{ecs::message::Messages, input::InputPlugin};

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(InputPlugin)
            .register_type::<KeyCode>()
            .add_message::<WindowEvent>()
            .add_message::<WindowFocused>();
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        app
    }

    fn send(app: &mut App, source: &str) -> Result<(), String> {
        let lua = mlua::Lua::new();
        let args: Table = lua.load(format!("return {source}")).eval().unwrap();
        deliver(app.world_mut(), &args)
    }

    #[test]
    fn raw_gamepad_inputs_are_processed_by_bevys_input_systems() {
        let mut app = app();
        send(
            &mut app,
            "{kind='gamepad_connect', gamepad='player', name='Controller'}",
        )
        .unwrap();
        let entity = app.world().resource::<Gamepads>().0["player"].0;
        assert!(app.world().get::<Gamepad>(entity).is_none());
        send(
            &mut app,
            "{kind='gamepad_button', gamepad='player', button='South', value=1}",
        )
        .unwrap();
        send(
            &mut app,
            "{kind='gamepad_axis', gamepad='player', axis='LeftStickX', value=-0.8}",
        )
        .unwrap();
        app.update();
        let gamepad = app.world().get::<Gamepad>(entity).unwrap();
        assert!(gamepad.just_pressed(GamepadButton::South));
        assert_eq!(gamepad.get(GamepadAxis::LeftStickX), Some(-0.8));
        assert_eq!(
            app.world().get::<Name>(entity).unwrap().as_str(),
            "Controller"
        );
        app.update();
        assert!(
            !app.world()
                .get::<Gamepad>(entity)
                .unwrap()
                .just_pressed(GamepadButton::South)
        );
        send(
            &mut app,
            "{kind='gamepad_button', gamepad='player', button='South', value=0}",
        )
        .unwrap();
        app.update();
        assert!(
            app.world()
                .get::<Gamepad>(entity)
                .unwrap()
                .just_released(GamepadButton::South)
        );
        send(&mut app, "{kind='gamepad_disconnect', gamepad='player'}").unwrap();
        app.update();
        assert!(app.world().get::<Gamepad>(entity).is_none());
        assert!(
            send(
                &mut app,
                "{kind='gamepad_axis', gamepad='player', axis='LeftStickX', value=1}"
            )
            .is_err()
        );
        send(&mut app, "{kind='gamepad_connect', gamepad='player'}").unwrap();
        app.update();
        assert_eq!(app.world().resource::<Gamepads>().0["player"].0, entity);
        assert!(app.world().get::<Gamepad>(entity).is_some());
    }

    #[test]
    fn gamepad_validation_rejects_unknown_controls_devices_and_invalid_values() {
        let mut app = app();
        send(&mut app, "{kind='gamepad_connect', gamepad='player'}").unwrap();
        for input in [
            "{kind='gamepad_connect', gamepad='player'}",
            "{kind='gamepad_disconnect', gamepad='missing'}",
            "{kind='gamepad_axis', gamepad='missing', axis='LeftStickX', value=1}",
            "{kind='gamepad_axis', gamepad='player', axis='Unknown', value=1}",
            "{kind='gamepad_axis', gamepad='player', axis='LeftStickX', value=2}",
            "{kind='gamepad_axis', gamepad='player', axis='LeftStickX', value=0/0}",
            "{kind='gamepad_button', gamepad='player', button='Unknown', value=1}",
            "{kind='gamepad_button', gamepad='player', button='South', value=-1}",
        ] {
            assert!(send(&mut app, input).is_err(), "{input}");
        }
    }

    #[test]
    fn focus_events_update_the_window_and_release_keyboard_input() {
        let mut app = app();
        send(&mut app, "{kind='key', key='Space', down=true}").unwrap();
        app.update();
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::Space)
        );
        send(&mut app, "{kind='focus', focused=false}").unwrap();
        let events = app.world().resource::<Messages<WindowFocused>>();
        assert!(!events.get_cursor().read(events).next().unwrap().focused);
        app.update();
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::Space)
        );
        send(&mut app, "{kind='focus', focused=true}").unwrap();
        app.update();
        let window = app
            .world_mut()
            .query::<&Window>()
            .single(app.world())
            .unwrap();
        assert!(window.focused);
    }
}
