use std::collections::HashMap;

use bevy::{
    ecs::system::SystemParam,
    picking::{
        backend::HitData,
        hover::{HoverMap, Hovered},
        pointer::{PointerId, PointerLocation},
    },
    prelude::*,
    ui::InteractionDisabled,
    window::{CursorIcon, PrimaryWindow, SystemCursorIcon},
};

use crate::{KuiAction, KuiSystems, KuiView, KuiViewStatus, runtime::ActionBindings};

/// Geometry shared by click, hover, press/release and drag actions.
/// Positions are in the receiving element's parent coordinates. `start` is the
/// press position for a drag.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct KuiActionData {
    pub position: Option<Vec2>,
    pub start: Option<Vec2>,
}

pub(crate) fn is_event_attribute(name: &str) -> bool {
    matches!(
        name,
        "on-click"
            | "on-press"
            | "on-release"
            | "on-hover-enter"
            | "on-hover"
            | "on-hover-exit"
            | "on-drag-start"
            | "on-drag"
            | "on-drag-end"
            | "on-drag-cancel"
    )
}

#[derive(Component)]
pub(crate) struct HoverCursor(pub SystemCursorIcon);
#[derive(Component)]
struct SuppressClick;

#[derive(Clone)]
struct Drag {
    source: Entity,
    target: Entity,
    camera: Entity,
    start: Option<Vec2>,
    position: Option<Vec2>,
    started: bool,
    cancel: Option<KuiAction>,
}

#[derive(Resource, Default)]
struct Drags(HashMap<PointerId, Drag>);

#[derive(Resource, Default)]
struct Hovers(HashMap<(Entity, PointerId), Hover>);

struct Hover {
    data: KuiActionData,
    exit: Option<KuiAction>,
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<Drags>()
        .init_resource::<Hovers>()
        .add_observer(click)
        .add_observer(press)
        .add_observer(release)
        .add_observer(drag_start)
        .add_observer(drag_move)
        .add_observer(drag_end)
        .add_observer(cancel)
        .add_systems(
            Update,
            (cancel_unavailable, hover, cursor)
                .chain()
                .in_set(KuiSystems::Input)
                .before(KuiSystems::Prepare),
        )
        .add_systems(
            PreUpdate,
            text_hits.in_set(bevy::picking::PickingSystems::Backend),
        );
}

#[derive(SystemParam)]
pub(crate) struct Actions<'w, 's> {
    bindings: Query<'w, 's, &'static ActionBindings>,
    views: Query<'w, 's, &'static KuiView>,
    entities: Query<'w, 's, ()>,
    parents: Query<'w, 's, &'static ChildOf>,
    globals: Query<'w, 's, &'static GlobalTransform>,
    layout: Query<'w, 's, (&'static UiGlobalTransform, &'static ComputedNode)>,
    cameras: Query<'w, 's, (&'static Camera, &'static GlobalTransform)>,
    availability: Query<
        'w,
        's,
        (
            Option<&'static Visibility>,
            Option<&'static Node>,
            Option<&'static crate::runtime::BoundVisibility>,
            Has<InteractionDisabled>,
        ),
    >,
}

impl Actions<'_, '_> {
    pub(crate) fn enabled(&self, target: Entity) -> bool {
        let Ok(binding) = self.bindings.get(target) else {
            return false;
        };
        let mut cursor = Some(target);
        while let Some(entity) = cursor {
            if let Ok((visibility, node, bound_visibility, disabled)) =
                self.availability.get(entity)
                && (disabled
                    || visibility == Some(&Visibility::Hidden)
                    || bound_visibility.is_some_and(|visibility| !visibility.visible)
                    || node.is_some_and(|node| node.display == Display::None))
            {
                return false;
            }
            cursor = self.parents.get(entity).ok().map(ChildOf::parent);
        }
        binding.value_valid
            && self.views.get(binding.view).is_ok_and(|view| {
                view.status() == &KuiViewStatus::Ready && self.entities.contains(view.owner)
            })
    }

    fn action(&self, target: Entity, event: &str, data: KuiActionData) -> Option<KuiAction> {
        let binding = self.bindings.get(target).ok()?;
        let action = binding
            .events
            .get(event)
            .filter(|action| !action.is_empty())?;
        let view = self.views.get(binding.view).ok()?;
        Some(KuiAction {
            entity: view.owner,
            view: binding.view,
            action: action.clone(),
            value: binding.value.clone(),
            data,
        })
    }

    pub(crate) fn emit(
        &self,
        commands: &mut Commands,
        target: Entity,
        event: &str,
        data: KuiActionData,
    ) {
        if let Some(action) = self.action(target, event, data) {
            commands.trigger(action);
        }
    }

    fn cancel_drag(&self, commands: &mut Commands, drag: &Drag) {
        // Retain the gesture's cancellation route so reloading/removing its
        // geometry can still finish the interaction on the surviving controller.
        if let Some(mut action) = drag.cancel.clone()
            && self.entities.contains(action.entity)
        {
            action.data = KuiActionData {
                position: drag.position,
                start: drag.start,
            };
            commands.trigger(action);
        }
    }

    fn position(
        &self,
        target: Entity,
        screen: Option<Vec2>,
        camera: Entity,
        hit: Option<Vec3>,
    ) -> Option<Vec2> {
        let binding = self.bindings.get(target).ok()?;
        let view = self.views.get(binding.view).ok()?;
        let parent = self.parents.get(target).ok().map(ChildOf::parent);
        let position = if view.world_space {
            let inverse = self.globals.get(parent?).ok()?.affine().inverse();
            if let Some(hit) = hit {
                inverse.transform_point3(hit).truncate()
            } else {
                let (camera, transform) = self.cameras.get(camera).ok()?;
                let ray = camera.viewport_to_world(transform, screen?).ok()?;
                let origin = inverse.transform_point3(ray.origin);
                let direction = inverse.transform_vector3(*ray.direction);
                if direction.z.abs() < f32::EPSILON {
                    return None;
                }
                let plane = inverse
                    .transform_point3(self.globals.get(target).ok()?.translation())
                    .z;
                (origin + direction * ((plane - origin.z) / direction.z)).truncate()
            }
        } else {
            let (camera, _) = self.cameras.get(camera).ok()?;
            let physical = screen? * camera.target_scaling_factor()?
                - camera.physical_viewport_rect()?.min.as_vec2();
            if let Some(parent) = parent {
                let (transform, computed) = self.layout.get(parent).ok()?;
                transform.try_inverse()?.transform_point2(physical)
                    * computed.inverse_scale_factor()
            } else {
                physical * self.layout.get(target).ok()?.1.inverse_scale_factor()
            }
        };
        position.is_finite().then_some(position)
    }
}

fn click(
    mut event: On<PointerClick>,
    actions: Actions,
    suppressed: Query<(), With<SuppressClick>>,
    mut commands: Commands,
) {
    let Ok(binding) = actions.bindings.get(event.entity) else {
        return;
    };
    if !binding.events.contains_key("on-click") && !listens(binding, "on-drag") {
        return;
    }
    event.propagate(false);
    if event.button != PointerButton::Primary
        || !actions.enabled(event.entity)
        || suppressed.contains(event.entity)
        || suppressed.contains(event.original_event_target())
    {
        return;
    }
    let position = actions.position(
        event.entity,
        Some(event.pointer.position),
        event.hit.camera,
        event.hit.position,
    );
    actions.emit(
        &mut commands,
        event.entity,
        "on-click",
        KuiActionData {
            position,
            start: None,
        },
    );
}

fn press(mut event: On<PointerPress>, actions: Actions, mut commands: Commands) {
    // A new press starts a new gesture, including after the previous drag ended.
    if event.entity == event.original_event_target() && event.button == PointerButton::Primary {
        let mut cursor = Some(event.entity);
        while let Some(entity) = cursor {
            commands.entity(entity).try_remove::<SuppressClick>();
            cursor = actions.parents.get(entity).ok().map(ChildOf::parent);
        }
    }
    let Ok(binding) = actions.bindings.get(event.entity) else {
        return;
    };
    if !binding.events.contains_key("on-press") {
        return;
    }
    event.propagate(false);
    if event.button != PointerButton::Primary || !actions.enabled(event.entity) {
        return;
    }
    let position = actions.position(
        event.entity,
        Some(event.pointer.position),
        event.hit.camera,
        event.hit.position,
    );
    actions.emit(
        &mut commands,
        event.entity,
        "on-press",
        KuiActionData {
            position,
            start: None,
        },
    );
}

fn release(mut event: On<PointerRelease>, actions: Actions, mut commands: Commands) {
    let Ok(binding) = actions.bindings.get(event.entity) else {
        return;
    };
    if !binding.events.contains_key("on-release") {
        return;
    }
    event.propagate(false);
    if event.button != PointerButton::Primary || !actions.enabled(event.entity) {
        return;
    }
    let position = actions.position(
        event.entity,
        Some(event.pointer.position),
        event.hit.camera,
        event.hit.position,
    );
    actions.emit(
        &mut commands,
        event.entity,
        "on-release",
        KuiActionData {
            position,
            start: None,
        },
    );
}

fn drag_start(
    mut event: On<PointerDragStart>,
    actions: Actions,
    mut drags: ResMut<Drags>,
    mut commands: Commands,
) {
    let Ok(binding) = actions.bindings.get(event.entity) else {
        return;
    };
    if !listens(binding, "on-drag") {
        return;
    }
    event.propagate(false);
    if event.button != PointerButton::Primary || !actions.enabled(event.entity) {
        return;
    }
    let drag = Drag {
        source: event.original_event_target(),
        target: event.entity,
        camera: event.hit.camera,
        start: None,
        position: None,
        started: false,
        cancel: actions.action(event.entity, "on-drag-cancel", KuiActionData::default()),
    };
    commands.entity(drag.source).insert(SuppressClick);
    commands.entity(drag.target).insert(SuppressClick);
    drags.0.insert(event.pointer.id, drag);
}

fn drag_move(
    mut event: On<PointerDrag>,
    actions: Actions,
    mut drags: ResMut<Drags>,
    mut commands: Commands,
) {
    let Some(drag) = drags.0.get_mut(&event.pointer.id) else {
        return;
    };
    if event.entity != drag.target || event.original_event_target() != drag.source {
        return;
    }
    event.propagate(false);
    if event.button != PointerButton::Primary || !actions.enabled(drag.target) {
        return;
    }
    if !drag.started {
        drag.start = actions.position(
            drag.target,
            Some(event.pointer.position - event.distance),
            drag.camera,
            None,
        );
        drag.started = true;
        actions.emit(
            &mut commands,
            drag.target,
            "on-drag-start",
            KuiActionData {
                position: drag.start,
                start: drag.start,
            },
        );
    }
    drag.position = actions.position(drag.target, Some(event.pointer.position), drag.camera, None);
    actions.emit(
        &mut commands,
        drag.target,
        "on-drag",
        KuiActionData {
            position: drag.position,
            start: drag.start,
        },
    );
}

fn drag_end(
    mut event: On<PointerDragEnd>,
    actions: Actions,
    mut drags: ResMut<Drags>,
    mut commands: Commands,
) {
    let Some(drag) = drags.0.get(&event.pointer.id) else {
        return;
    };
    if event.entity != drag.target
        || event.original_event_target() != drag.source
        || event.button != PointerButton::Primary
    {
        return;
    }
    event.propagate(false);
    let drag = drags.0.remove(&event.pointer.id).unwrap();
    let position = actions.position(drag.target, Some(event.pointer.position), drag.camera, None);
    if actions.enabled(drag.target) {
        actions.emit(
            &mut commands,
            drag.target,
            "on-drag-end",
            KuiActionData {
                position,
                start: drag.start,
            },
        );
    } else {
        actions.cancel_drag(&mut commands, &drag);
    }
}

fn cancel(
    event: On<PointerCancel>,
    actions: Actions,
    mut drags: ResMut<Drags>,
    mut commands: Commands,
) {
    if let Some(drag) = drags.0.remove(&event.pointer.id) {
        actions.cancel_drag(&mut commands, &drag);
    }
}

fn cancel_unavailable(
    actions: Actions,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut drags: ResMut<Drags>,
    mut commands: Commands,
) {
    let unfocused = windows.iter().any(|window| !window.focused);
    drags.0.retain(|_, drag| {
        if unfocused || !actions.enabled(drag.target) || !actions.entities.contains(drag.source) {
            actions.cancel_drag(&mut commands, drag);
            false
        } else {
            true
        }
    });
}

fn hover(
    actions: Actions,
    map: Option<Res<HoverMap>>,
    locations: Query<(&PointerId, &PointerLocation)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut hovers: ResMut<Hovers>,
    mut commands: Commands,
) {
    let mut current = HashMap::new();
    if !windows.iter().any(|window| !window.focused)
        && let Some(map) = map
    {
        for (id, hits) in &map.0 {
            for (source, hit) in hits {
                let mut cursor = Some(*source);
                while let Some(target) = cursor {
                    if let Ok(binding) = actions.bindings.get(target)
                        && listens(binding, "on-hover")
                    {
                        if actions.enabled(target) {
                            let screen = locations
                                .iter()
                                .find(|(pointer, _)| *pointer == id)
                                .and_then(|(_, location)| location.location.as_ref())
                                .map(|location| location.position);
                            let position =
                                actions.position(target, screen, hit.camera, hit.position);
                            current.entry((target, *id)).or_insert_with(|| Hover {
                                data: KuiActionData {
                                    position,
                                    start: None,
                                },
                                exit: actions.action(
                                    target,
                                    "on-hover-exit",
                                    KuiActionData::default(),
                                ),
                            });
                        }
                        break;
                    }
                    cursor = actions.parents.get(target).ok().map(ChildOf::parent);
                }
            }
        }
    }
    for ((target, id), hover) in &current {
        let previous = hovers.0.get(&(*target, *id));
        if previous.is_none() {
            actions.emit(&mut commands, *target, "on-hover-enter", hover.data);
        }
        if previous.map(|previous| previous.data) != Some(hover.data) {
            actions.emit(&mut commands, *target, "on-hover", hover.data);
        }
    }
    for (key, previous) in &hovers.0 {
        if !current.contains_key(key)
            && let Some(exit) = &previous.exit
            && actions.entities.contains(exit.entity)
        {
            commands.trigger(exit.clone());
        }
    }
    hovers.0 = current;
}

fn listens(bindings: &ActionBindings, prefix: &str) -> bool {
    bindings.events.keys().any(|key| key.starts_with(prefix))
}

fn cursor(
    mut commands: Commands,
    windows: Query<(Entity, Option<&CursorIcon>), With<PrimaryWindow>>,
    hovered: Query<(&HoverCursor, &Hovered), Without<InteractionDisabled>>,
    drags: Res<Drags>,
) {
    let Ok((window, current)) = windows.single() else {
        return;
    };
    if current.is_some_and(|icon| icon.as_system().is_none()) {
        return;
    }
    let icon = CursorIcon::System(if !drags.0.is_empty() {
        SystemCursorIcon::Grabbing
    } else {
        hovered
            .iter()
            .find(|(_, hovered)| hovered.get())
            .map_or(SystemCursorIcon::Default, |(cursor, _)| cursor.0)
    });
    if current != Some(&icon) {
        commands.entity(window).insert(icon);
    }
}

/// Bevy supplies sprite/UI backends; world text uses its measured layout bounds.
fn text_hits(
    rays: Option<Res<bevy::picking::backend::ray::RayMap>>,
    mut output: Option<ResMut<Messages<bevy::picking::backend::PointerHits>>>,
    cameras: Query<(
        &Camera,
        &GlobalTransform,
        Option<&bevy::camera::visibility::RenderLayers>,
    )>,
    texts: Query<
        (
            Entity,
            &bevy::text::TextLayoutInfo,
            &bevy::sprite::Anchor,
            &GlobalTransform,
            &ViewVisibility,
            &Pickable,
            Option<&bevy::camera::visibility::RenderLayers>,
        ),
        With<Text2d>,
    >,
) {
    let (Some(rays), Some(output)) = (rays, output.as_mut()) else {
        return;
    };
    for (id, ray) in rays.iter() {
        let Ok((camera, camera_transform, layers)) = cameras.get(id.camera) else {
            continue;
        };
        let mut hits = Vec::new();
        for (entity, layout, anchor, transform, visibility, pickable, text_layers) in &texts {
            if !visibility.get()
                || !pickable.is_hoverable
                || !layers
                    .unwrap_or_default()
                    .intersects(text_layers.unwrap_or_default())
            {
                continue;
            }
            let inverse = transform.affine().inverse();
            let origin = inverse.transform_point3(ray.origin);
            let direction = inverse.transform_vector3(*ray.direction);
            if !direction.is_finite() || direction.z.abs() < f32::EPSILON {
                continue;
            }
            let distance = -origin.z / direction.z;
            if distance < 0.0 {
                continue;
            }
            let point = origin + direction * distance;
            let size = layout.size;
            let min = -(anchor.as_vec() + Vec2::splat(0.5)) * size;
            if Rect::from_corners(min, min + size).contains(point.truncate()) {
                let world = ray.get_point(distance);
                let depth = -camera_transform
                    .affine()
                    .inverse()
                    .transform_point3(world)
                    .z;
                hits.push((entity, HitData::new(id.camera, depth, Some(world), None)));
            }
        }
        output.write(bevy::picking::backend::PointerHits::new(
            id.pointer,
            hits,
            camera.order as f32,
        ));
    }
}

#[cfg(test)]
mod tests;
