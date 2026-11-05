use bevy::{
    asset::{RenderAssetUsages, uuid::Uuid},
    camera::RenderTarget,
    picking::pointer::{Location, PointerAction, PointerId, PointerInput},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
    window::PrimaryWindow,
};

use crate::Workbench;

#[derive(Component)]
pub(crate) struct ChromeNode;

const POINTER: PointerId = PointerId::Custom(Uuid::from_u128(0x6b75695f776f726b62656e6368));

pub(crate) fn setup(world: &mut World, tool: &mut Workbench) {
    let mut image = Image::new_fill(
        Extent3d {
            width: 800,
            height: 600,
            ..default()
        },
        TextureDimension::D2,
        &[24, 24, 24, 255],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    tool.image = world.resource_mut::<Assets<Image>>().add(image);
    tool.preview_camera = Some(
        world
            .spawn((
                Camera2d,
                Camera {
                    order: -1,
                    clear_color: ClearColorConfig::Custom(Color::srgb(0.10, 0.12, 0.14)),
                    ..default()
                },
                RenderTarget::Image(tool.image.clone().into()),
            ))
            .id(),
    );
    world.spawn(POINTER);
}

pub(crate) fn attach(
    mut commands: Commands,
    tool: Res<Workbench>,
    names: Query<(Entity, &Name), Added<Name>>,
    parents: Query<&ChildOf>,
) {
    for (entity, name) in &names {
        let mut ancestor = entity;
        let is_chrome = loop {
            if Some(ancestor) == tool.chrome_mount {
                break true;
            }
            if let Ok(parent) = parents.get(ancestor) {
                ancestor = parent.parent();
            } else {
                break false;
            }
        };
        if !is_chrome {
            continue;
        }
        commands.entity(entity).insert(ChromeNode);
        if name.as_str() == "preview-image" {
            commands
                .entity(entity)
                .insert((ImageNode::new(tool.image.clone()), Pickable::IGNORE));
        }
        if matches!(name.as_str(), "views-pane" | "bindings-list" | "action-log") {
            commands.entity(entity).insert(Pickable::default());
        }
    }
}

pub(crate) fn layout(
    mut tool: ResMut<Workbench>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut nodes: Query<(&Name, &mut Node, &ComputedNode, &UiGlobalTransform), With<ChromeNode>>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<&mut Camera>,
) {
    let Ok(window) = windows.single() else { return };
    let scale = window.scale_factor();
    let mut available = Vec2::ZERO;
    for (name, mut node, computed, transform) in &mut nodes {
        match name.as_str() {
            "preview-area" => {
                available = computed.size() / scale;
                tool.preview_clip_rect =
                    Rect::from_center_size(transform.translation / scale, available);
            }
            "views-pane" => node.width = px(tool.left_width.min(window.width() * 0.3)),
            "inspector-pane" => node.width = px(tool.right_width.min(window.width() * 0.45)),
            _ => {}
        }
    }
    if available.min_element() < 1. {
        return;
    }
    let resolution = tool
        .viewport
        .unwrap_or_else(|| available.as_uvec2().max(UVec2::ONE))
        .min(UVec2::splat(4096));
    if resolution != tool.resolution {
        if let Some(mut image) = images.get_mut(&tool.image) {
            image.resize(Extent3d {
                width: resolution.x,
                height: resolution.y,
                ..default()
            });
        }
        tool.resolution = resolution;
    }
    let display = display_size(available, resolution, tool.fit || tool.viewport.is_none());
    for (name, mut node, computed, transform) in &mut nodes {
        if name.as_str() == "preview-image" {
            if node.width != px(display.x) {
                node.width = px(display.x);
            }
            if node.height != px(display.y) {
                node.height = px(display.y);
            }
            node.flex_shrink = 0.;
            let center = transform.translation / scale;
            tool.preview_rect = Rect::from_center_size(center, computed.size() / scale);
        }
    }
    if let Some(camera) = tool.preview_camera
        && let Ok(mut camera) = cameras.get_mut(camera)
    {
        camera.clear_color = ClearColorConfig::Custom(match tool.background {
            1 => Color::srgb(0.7, 0.7, 0.7),
            2 => Color::BLACK,
            _ => Color::srgb(0.10, 0.12, 0.14),
        });
    }
}

fn display_size(available: Vec2, resolution: UVec2, fit: bool) -> Vec2 {
    let size = resolution.as_vec2();
    if fit {
        size * (available / size).min_element()
    } else {
        size
    }
}

pub(crate) fn pointer(
    tool: Res<Workbench>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    mut inputs: ParamSet<(MessageReader<PointerInput>, MessageWriter<PointerInput>)>,
    mut last: Local<Vec2>,
    mut captured: Local<[bool; 3]>,
) {
    let Ok((window_entity, window)) = windows.single() else {
        return;
    };
    let target = RenderTarget::Image(tool.image.clone().into())
        .normalize(Some(window_entity))
        .unwrap();
    let window_target = RenderTarget::Window(bevy::window::WindowRef::Entity(window_entity))
        .normalize(Some(window_entity))
        .unwrap();
    let mut mapped = Vec::new();
    for input in inputs.p0().read() {
        if input.pointer_id != PointerId::Mouse || input.location.target != window_target {
            continue;
        }
        let cursor = input.location.position;
        let inside = tool.preview_rect.size().min_element() > 0.
            && tool.preview_rect.contains(cursor)
            && tool.preview_clip_rect.contains(cursor);
        let position = if inside {
            map_pointer(tool.preview_rect, tool.resolution, cursor)
        } else {
            Vec2::splat(-100_000.)
        };
        let action = match input.action {
            PointerAction::Press(button) if inside => {
                let index = button_index(button);
                captured[index] = true;
                PointerAction::Press(button)
            }
            PointerAction::Release(button) if captured[button_index(button)] => {
                captured[button_index(button)] = false;
                PointerAction::Release(button)
            }
            PointerAction::Move { .. } => PointerAction::Move {
                delta: position - *last,
            },
            PointerAction::Scroll { .. } if inside => input.action,
            PointerAction::Cancel => {
                *captured = [false; 3];
                PointerAction::Cancel
            }
            _ => continue,
        };
        *last = position;
        mapped.push(PointerInput::new(
            POINTER,
            Location {
                target: target.clone(),
                position,
            },
            action,
        ));
    }
    let cursor = window.cursor_position();
    let position = cursor
        .filter(|position| {
            tool.preview_rect.size().min_element() > 0.
                && tool.preview_rect.contains(*position)
                && tool.preview_clip_rect.contains(*position)
        })
        .map(|position| map_pointer(tool.preview_rect, tool.resolution, position))
        .unwrap_or(Vec2::splat(-100_000.));
    if position != *last {
        mapped.push(PointerInput::new(
            POINTER,
            Location { target, position },
            PointerAction::Move {
                delta: position - *last,
            },
        ));
        *last = position;
    }
    inputs.p1().write_batch(mapped);
}

fn map_pointer(rect: Rect, resolution: UVec2, position: Vec2) -> Vec2 {
    (position - rect.min) / rect.size() * resolution.as_vec2()
}

fn button_index(button: PointerButton) -> usize {
    match button {
        PointerButton::Primary => 0,
        PointerButton::Secondary => 1,
        PointerButton::Middle => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_pointer_preserves_capture_scroll_and_source_events_without_feedback() {
        let mut app = App::new();
        app.add_message::<PointerInput>()
            .add_systems(First, pointer);
        let mut window = Window::default();
        window.set_cursor_position(Some(Vec2::new(150., 100.)));
        let window = app.world_mut().spawn((window, PrimaryWindow)).id();
        let window_target = RenderTarget::Window(bevy::window::WindowRef::Entity(window))
            .normalize(Some(window))
            .unwrap();
        let mut tool = Workbench::new("unused".into());
        tool.preview_rect = Rect::from_corners(Vec2::new(100., 50.), Vec2::new(300., 150.));
        tool.preview_clip_rect = tool.preview_rect;
        tool.resolution = UVec2::new(800, 400);
        app.insert_resource(tool);
        for action in [
            PointerAction::Move { delta: Vec2::ONE },
            PointerAction::Press(PointerButton::Primary),
            PointerAction::Scroll {
                unit: bevy::input::mouse::MouseScrollUnit::Line,
                x: 1.,
                y: -2.,
                phase: bevy::input::touch::TouchPhase::Moved,
            },
        ] {
            app.world_mut().write_message(PointerInput::new(
                PointerId::Mouse,
                Location {
                    target: window_target.clone(),
                    position: Vec2::new(150., 100.),
                },
                action,
            ));
        }
        app.update();
        let events: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<PointerInput>>()
            .drain()
            .collect();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.pointer_id == PointerId::Mouse)
                .count(),
            3
        );
        let mapped: Vec<_> = events
            .iter()
            .filter(|event| event.pointer_id == POINTER)
            .collect();
        assert_eq!(mapped.len(), 3);
        assert!(
            mapped
                .iter()
                .all(|event| event.location.position == Vec2::new(200., 200.))
        );
        assert!(matches!(
            mapped[2].action,
            PointerAction::Scroll { x: 1., y: -2., .. }
        ));
        app.update();
        assert!(app.world().resource::<Messages<PointerInput>>().is_empty());

        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(Some(Vec2::ZERO));
        app.world_mut().write_message(PointerInput::new(
            PointerId::Mouse,
            Location {
                target: window_target,
                position: Vec2::ZERO,
            },
            PointerAction::Release(PointerButton::Primary),
        ));
        app.update();
        let events: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<PointerInput>>()
            .drain()
            .collect();
        assert!(events.iter().any(|event| event.pointer_id == POINTER
            && matches!(event.action, PointerAction::Release(PointerButton::Primary))));
    }

    #[test]
    fn fitting_preserves_authored_resolution_and_maps_scaled_pointer() {
        let resolution = UVec2::new(1920, 1080);
        let display = display_size(Vec2::new(800., 600.), resolution, true);
        assert_eq!(display, Vec2::new(800., 450.));
        let rect = Rect::from_corners(Vec2::new(180., 100.), Vec2::new(980., 550.));
        assert_eq!(
            map_pointer(rect, resolution, Vec2::new(580., 325.)),
            Vec2::new(960., 540.)
        );
        assert_eq!(
            display_size(Vec2::new(800., 600.), resolution, false),
            resolution.as_vec2()
        );
    }
}
