use super::*;
use bevy::{
    camera::{CameraProjection, ComputedCameraValues, NormalizedRenderTarget, RenderTargetInfo},
    picking::pointer::Location,
};

#[derive(Resource, Default)]
struct Recorded(Vec<KuiAction>);

fn fixture() -> (App, Entity, Entity, Entity, Entity) {
    let mut app = crate::runtime::test_support::app(&[]);
    app.init_resource::<Recorded>()
        .init_resource::<HoverMap>()
        .add_observer(|event: On<KuiAction>, mut recorded: ResMut<Recorded>| {
            recorded.0.push(event.clone())
        });
    let source = format!(
        r#"{}<node id="frame" on-click="outer">
      <node id="target" on-click="click" on-hover-enter="enter" on-hover="hover" on-hover-exit="exit"
        on-drag-start="start" on-drag="move" on-drag-end="end" on-drag-cancel="cancel"
        value="item" bind:position="pose">
        <text id="a">A</text><text id="b">B</text>
      </node></node></view>"#,
        crate::tests::HEADER
    );
    let definition = crate::parse("input.kui", &source).unwrap();
    let asset = app
        .world_mut()
        .resource_mut::<Assets<crate::KuiAsset>>()
        .add(crate::KuiAsset {
            definition,
            style_sheet: None,
            image_paths: vec![],
            image_handles: vec![],
            component_style_paths: vec![],
            component_style_handles: vec![],
        });
    let owner = app.world_mut().spawn_empty().id();
    let mut view = KuiView::new(owner, asset).in_world();
    view.data.insert("pose".into(), pose(5., 6., 7.));
    let mount = app.world_mut().spawn(view).id();
    app.update();
    let frame = named(&mut app, "frame");
    app.world_mut()
        .entity_mut(frame)
        .insert(GlobalTransform::from(
            Transform::from_xyz(10., 20., 0.)
                .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
                .with_scale(Vec3::splat(2.)),
        ));
    let camera = app
        .world_mut()
        .spawn((
            Camera {
                computed: ComputedCameraValues {
                    clip_from_view: OrthographicProjection {
                        area: Rect::new(-400., -300., 400., 300.),
                        ..OrthographicProjection::default_2d()
                    }
                    .get_clip_from_view(),
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(800, 600),
                        scale_factor: 1.,
                    }),
                    ..default()
                },
                ..default()
            },
            GlobalTransform::from_translation(Vec3::new(0., 0., 100.)),
        ))
        .id();
    let target = named(&mut app, "target");
    let source = named(&mut app, "a");
    (app, mount, target, source, camera)
}

fn pose(x: f64, y: f64, z: f64) -> crate::KuiValue {
    crate::KuiValue::Record(std::collections::BTreeMap::from([
        ("x".into(), crate::KuiValue::Number(x)),
        ("y".into(), crate::KuiValue::Number(y)),
        ("z".into(), crate::KuiValue::Number(z)),
    ]))
}

fn named(app: &mut App, name: &str) -> Entity {
    app.world_mut()
        .query::<(Entity, &Name)>()
        .iter(app.world())
        .find(|(_, candidate)| candidate.as_str() == name)
        .unwrap()
        .0
}

fn begin_drag(app: &mut App, source: Entity, camera: Entity) {
    let start = Vec2::new(10., 20.);
    let end = Vec2::new(30., 40.);
    let start_screen = world_screen(app, camera, start);
    let end_screen = world_screen(app, camera, end);
    app.world_mut().trigger(PointerDragStart {
        entity: source,
        pointer: pointer(end_screen),
        button: PointerButton::Primary,
        hit: HitData::new(camera, 1., Some(start.extend(0.)), None),
    });
    app.world_mut().trigger(PointerDrag {
        entity: source,
        pointer: pointer(end_screen),
        button: PointerButton::Primary,
        distance: end_screen - start_screen,
        delta: end_screen - start_screen,
    });
    app.world_mut().flush();
}

fn world_screen(app: &App, camera: Entity, point: Vec2) -> Vec2 {
    app.world()
        .get::<Camera>(camera)
        .unwrap()
        .world_to_viewport(
            app.world().get::<GlobalTransform>(camera).unwrap(),
            point.extend(0.),
        )
        .unwrap()
}

fn pointer(position: Vec2) -> Pointer {
    Pointer::new(
        PointerId::Mouse,
        Location {
            target: NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position,
        },
    )
}

fn assert_position(actual: Option<Vec2>, expected: Vec2) {
    assert!(
        actual.unwrap().abs_diff_eq(expected, 0.001),
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn event_vocabulary_is_shared_by_all_visual_elements() {
    for event in [
        "click",
        "press",
        "release",
        "hover-enter",
        "hover",
        "hover-exit",
        "drag-start",
        "drag",
        "drag-end",
        "drag-cancel",
    ] {
        for element in ["node", "button", "text"] {
            let source = format!(
                "{}<{element} on-{event}=\"handle\"/></view>",
                crate::tests::HEADER
            );
            let definition = crate::parse("events.kui", &source).unwrap();
            assert_eq!(
                definition.children[0].attributes[&format!("on-{event}")],
                "handle"
            );
        }
    }
}

#[test]
fn interactive_elements_opt_into_bevy_hover_in_both_contexts() {
    let source = format!(
        r#"{}<node id="decorative">
          <node id="click" on-click="click"/>
          <node id="drag" on-drag-start="drag"/>
          <text id="hover" on-hover-enter="enter">Hover</text>
          <node id="cursor" cursor="grab"/>
          <button id="button"/>
        </node></view>"#,
        crate::tests::HEADER
    );
    for world_space in [false, true] {
        let mut app = crate::runtime::test_support::app(&[("hover.kui", &source)]);
        let owner = app.world_mut().spawn_empty().id();
        let asset = app.world().resource::<AssetServer>().load("hover.kui");
        let mut view = KuiView::new(owner, asset);
        view.world_space = world_space;
        let mount = app.world_mut().spawn(view).id();
        crate::tests::wait_for(&mut app, |app| {
            app.world().get::<KuiView>(mount).unwrap().status() == &KuiViewStatus::Ready
        });
        for name in ["click", "drag", "hover", "cursor", "button"] {
            let entity = named(&mut app, name);
            assert_eq!(
                app.world().get::<bevy::picking::hover::Hovered>(entity),
                Some(&bevy::picking::hover::Hovered(false)),
                "{name}, world_space={world_space}"
            );
        }
        let decorative = named(&mut app, "decorative");
        assert!(
            app.world()
                .get::<bevy::picking::hover::Hovered>(decorative)
                .is_none()
        );
    }
}

#[test]
fn world_component_hover_styles_and_cursor_share_bevy_state_through_descendants() {
    use bevy::picking::{
        backend::PointerHits,
        hover::Hovered,
        pointer::{PointerAction, PointerInput},
    };

    let view = format!(
        r#"{}<import name="surface" src="surface.kui"/>
        <use component="surface" id="surface">
          <prop name="disabled" bind:value="disabled"/>
          <on name="activate" action="activate"/>
          <fill slot="contents"><node id="geometry"/></fill>
        </use></view>"#,
        crate::tests::HEADER
    );
    let component = r#"<component xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="surface">
        <style src="surface.css"/>
        <prop name="disabled" type="boolean" default="false"/><event name="activate"/>
        <node class="surface" cursor="grab" on-click="emit.activate" bind:disabled="props.disabled">
          <slot name="contents"/><node id="feedback" class="feedback"/>
        </node></component>"#;
    let mut app = crate::tests::layout_app(&[
        ("hover.kui", &view),
        ("surface.kui", component),
        (
            "surface.css",
            ".feedback { scale: 1 1; } .surface:hover:not(:disabled) .feedback { scale: 2 2; }",
        ),
    ]);
    app.add_message::<bevy::window::WindowEvent>()
        .add_plugins(bevy_flair::FlairPlugin);
    app.finish();
    app.cleanup();
    let window = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    let owner = app.world_mut().spawn_empty().id();
    let asset = app.world().resource::<AssetServer>().load("hover.kui");
    let mut view = KuiView::new(owner, asset).in_world();
    view.data
        .insert("disabled".into(), crate::KuiValue::Boolean(false));
    let mount = app.world_mut().spawn(view).id();
    crate::tests::wait_for(&mut app, |app| {
        app.world().get::<KuiView>(mount).unwrap().status() == &KuiViewStatus::Ready
    });
    let surface = named(&mut app, "surface");
    let geometry = named(&mut app, "geometry");
    let feedback = named(&mut app, "feedback");
    app.world_mut()
        .entity_mut(geometry)
        .insert(Pickable::default());
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<Camera>>()
        .single(app.world())
        .unwrap();
    let hover = |app: &mut App, target: Option<Entity>| {
        app.world_mut().write_message(PointerInput::new(
            PointerId::Mouse,
            pointer(Vec2::splat(400.)).location(),
            PointerAction::Move { delta: Vec2::ZERO },
        ));
        app.world_mut().write_message(PointerHits::new(
            PointerId::Mouse,
            target
                .map(|entity| (entity, HitData::new(camera, 1., Some(Vec3::ZERO), None)))
                .into_iter()
                .collect(),
            100.,
        ));
        app.update();
    };
    let assert_state = |app: &App, hovered, scale, cursor| {
        assert_eq!(app.world().get::<Hovered>(surface), Some(&Hovered(hovered)));
        assert_eq!(
            app.world().get::<Transform>(feedback).unwrap().scale,
            Vec3::new(scale, scale, 1.)
        );
        assert_eq!(
            app.world().get::<CursorIcon>(window),
            Some(&CursorIcon::System(cursor))
        );
    };
    hover(&mut app, Some(geometry));
    assert_state(&app, true, 2., SystemCursorIcon::Grab);
    assert!(app.world().get::<Hovered>(geometry).is_none());
    hover(&mut app, Some(surface));
    assert_state(&app, true, 2., SystemCursorIcon::Grab);
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("disabled".into(), crate::KuiValue::Boolean(true));
    hover(&mut app, Some(geometry));
    hover(&mut app, Some(geometry));
    assert_state(&app, true, 1., SystemCursorIcon::Default);
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("disabled".into(), crate::KuiValue::Boolean(false));
    hover(&mut app, Some(geometry));
    hover(&mut app, Some(geometry));
    assert_state(&app, true, 2., SystemCursorIcon::Grab);
    hover(&mut app, None);
    assert_state(&app, false, 1., SystemCursorIcon::Default);
    hover(&mut app, None);
    assert_state(&app, false, 1., SystemCursorIcon::Default);
}

#[test]
fn drag_reports_geometry_without_owning_transforms_or_bindings_and_consumes_clicks() {
    let (mut app, mount, target, source, camera) = fixture();
    let before = *app.world().get::<Transform>(target).unwrap();
    begin_drag(&mut app, source, camera);
    let events = &app.world().resource::<Recorded>().0;
    assert_eq!(
        events.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(),
        ["start", "move"]
    );
    assert_position(events[0].data.start, Vec2::ZERO);
    assert_position(events[1].data.position, Vec2::new(10., -10.));
    assert_eq!(events[1].view, mount);
    assert_eq!(events[1].value.as_deref(), Some("item"));
    assert_eq!(*app.world().get::<Transform>(target).unwrap(), before);
    // The controller remains the sole position owner, including x/y during a drag.
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("pose".into(), pose(40., 50., 60.));
    app.update();
    assert_eq!(
        app.world().get::<Transform>(target).unwrap().translation,
        Vec3::new(40., 50., 60.)
    );
    crate::tests::click(&mut app, source, PointerButton::Primary);
    assert_eq!(
        app.world().resource::<Recorded>().0.len(),
        2,
        "a drag must not click its enclosing action"
    );
    let end_screen = world_screen(&app, camera, Vec2::new(70., 80.));
    app.world_mut().trigger(PointerDragEnd {
        entity: source,
        pointer: pointer(end_screen),
        button: PointerButton::Primary,
        distance: Vec2::new(60., -60.),
    });
    app.world_mut().flush();
    let end = app.world().resource::<Recorded>().0.last().unwrap();
    assert_eq!(end.action, "end");
    assert_position(end.data.position, Vec2::new(30., -30.));
    assert_position(end.data.start, Vec2::ZERO);
    // A fresh press resets click suppression, so a subsequent tap works.
    app.world_mut().trigger(PointerPress {
        entity: source,
        pointer: pointer(end_screen),
        button: PointerButton::Primary,
        hit: HitData::new(camera, 1., Some(Vec3::ZERO), None),
        count: 1,
    });
    app.world_mut().flush();
    crate::tests::click(&mut app, source, PointerButton::Primary);
    assert_eq!(
        app.world().resource::<Recorded>().0.last().unwrap().action,
        "click"
    );
}

#[test]
fn disabled_hidden_and_unfocused_drags_cancel_once_and_never_end_after_cancellation() {
    for reason in [
        "disabled",
        "hidden",
        "focus",
        "explicit",
        "source removed",
        "target removed",
    ] {
        let (mut app, _, target, source, camera) = fixture();
        begin_drag(&mut app, source, camera);
        match reason {
            "disabled" => {
                app.world_mut()
                    .entity_mut(target)
                    .insert(InteractionDisabled);
            }
            "hidden" => {
                app.world_mut()
                    .entity_mut(target)
                    .insert(Visibility::Hidden);
            }
            "focus" => {
                app.world_mut().spawn((
                    Window {
                        focused: false,
                        ..default()
                    },
                    PrimaryWindow,
                ));
            }
            "source removed" => {
                app.world_mut().despawn(source);
            }
            "target removed" => {
                app.world_mut().despawn(target);
            }
            _ => app.world_mut().trigger(PointerCancel {
                entity: source,
                pointer: pointer(Vec2::ZERO),
                hit: HitData::new(camera, 1., None, None),
            }),
        }
        app.update();
        app.update();
        assert_eq!(
            app.world()
                .resource::<Recorded>()
                .0
                .iter()
                .map(|e| e.action.as_str())
                .collect::<Vec<_>>(),
            ["start", "move", "cancel"],
            "{reason}"
        );
        if reason != "source removed" && reason != "target removed" {
            app.world_mut().trigger(PointerDragEnd {
                entity: source,
                pointer: pointer(Vec2::ZERO),
                button: PointerButton::Primary,
                distance: Vec2::ZERO,
            });
            assert_eq!(app.world().resource::<Recorded>().0.len(), 3, "{reason}");
        }
    }
}

fn set_hover(app: &mut App, source: Entity, camera: Entity, world: Vec2) {
    app.world_mut().resource_mut::<HoverMap>().0.insert(
        PointerId::Mouse,
        [(
            source,
            HitData::new(camera, 1., Some(world.extend(0.)), None),
        )]
        .into_iter()
        .collect(),
    );
}

#[test]
fn hover_tracks_the_handler_not_its_children_and_exits_once_on_occlusion_or_disable() {
    let (mut app, _, target, source, camera) = fixture();
    let sibling = named(&mut app, "b");
    set_hover(&mut app, source, camera, Vec2::new(10., 20.));
    app.update();
    assert_eq!(
        app.world()
            .resource::<Recorded>()
            .0
            .iter()
            .map(|e| e.action.as_str())
            .collect::<Vec<_>>(),
        ["enter", "hover"]
    );
    // Crossing child geometry inside one handler must not generate exit/enter.
    set_hover(&mut app, sibling, camera, Vec2::new(10., 20.));
    app.update();
    assert_eq!(app.world().resource::<Recorded>().0.len(), 2);
    // Picking updates intersections when a camera moves even without screen motion.
    set_hover(&mut app, sibling, camera, Vec2::new(30., 40.));
    app.update();
    let events = &app.world().resource::<Recorded>().0;
    assert_eq!(events.last().unwrap().action, "hover");
    assert_position(events.last().unwrap().data.position, Vec2::new(10., -10.));
    app.world_mut().resource_mut::<HoverMap>().0.clear();
    app.update();
    app.update();
    let events = &app.world().resource::<Recorded>().0;
    assert_eq!(
        events.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(),
        ["enter", "hover", "hover", "exit"]
    );
    assert_eq!(events.last().unwrap().data.position, None);
    set_hover(&mut app, source, camera, Vec2::new(10., 20.));
    app.update();
    app.world_mut()
        .entity_mut(target)
        .insert(InteractionDisabled);
    app.update();
    app.update();
    assert_eq!(
        app.world()
            .resource::<Recorded>()
            .0
            .iter()
            .filter(|e| e.action == "exit")
            .count(),
        2
    );
}

#[test]
fn text_picking_respects_geometry_visibility_and_layers() {
    use bevy::{
        camera::visibility::RenderLayers,
        picking::backend::{
            PointerHits,
            ray::{RayId, RayMap},
        },
        text::TextLayoutInfo,
    };
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<RayMap>()
        .add_message::<PointerHits>()
        .add_systems(Update, text_hits);
    let camera = app
        .world_mut()
        .spawn((
            Camera::default(),
            GlobalTransform::from_translation(Vec3::new(0., 0., 100.)),
        ))
        .id();
    app.world_mut().resource_mut::<RayMap>().map.insert(
        RayId::new(camera, PointerId::Mouse),
        Ray3d::new(Vec3::new(10., 20., 100.), Dir3::NEG_Z),
    );
    let text = app
        .world_mut()
        .spawn((
            Text2d::new("Hit"),
            TextLayoutInfo {
                size: Vec2::new(20., 10.),
                ..default()
            },
            bevy::sprite::Anchor::CENTER,
            GlobalTransform::from(
                Transform::from_xyz(10., 20., 2.)
                    .with_rotation(Quat::from_rotation_z(0.5))
                    .with_scale(Vec3::new(2., 3., 1.)),
            ),
            ViewVisibility::VISIBLE,
            Pickable::default(),
        ))
        .id();
    app.update();
    let hits: Vec<_> = app
        .world_mut()
        .resource_mut::<Messages<PointerHits>>()
        .drain()
        .collect();
    assert_eq!(hits[0].picks[0].0, text);
    assert_eq!(hits[0].picks[0].1.depth, 98.);
    app.world_mut()
        .entity_mut(text)
        .insert(ViewVisibility::HIDDEN);
    app.update();
    assert!(
        app.world_mut()
            .resource_mut::<Messages<PointerHits>>()
            .drain()
            .all(|hits| hits.picks.is_empty())
    );
    app.world_mut()
        .entity_mut(text)
        .insert(ViewVisibility::VISIBLE);
    app.world_mut()
        .entity_mut(camera)
        .insert(RenderLayers::layer(1));
    app.update();
    assert!(
        app.world_mut()
            .resource_mut::<Messages<PointerHits>>()
            .drain()
            .all(|hits| hits.picks.is_empty())
    );
}

#[test]
fn native_input_delivers_drag_outside_geometry_and_layout_hover_through_the_same_action_path() {
    use bevy::picking::pointer::{PointerAction, PointerInput};
    let source = format!(
        r#"{}<style src="input.css"/><node on-click="outer">
      <button id="target" on-click="click" on-press="press" on-release="release"
        on-drag-start="start" on-drag="move" on-drag-end="end" on-drag-cancel="cancel"
        on-hover-enter="enter" on-hover="hover" on-hover-exit="exit"/>
      </node></view>"#,
        crate::tests::HEADER
    );
    let mut app = crate::tests::layout_app(&[
        ("input.kui", &source),
        ("input.css", "button { width: 100px; height: 100px; }"),
    ]);
    app.add_message::<bevy::window::WindowEvent>()
        .add_plugins(bevy_flair::FlairPlugin)
        .init_resource::<Recorded>()
        .add_observer(|event: On<KuiAction>, mut recorded: ResMut<Recorded>| {
            recorded.0.push(event.clone())
        });
    app.finish();
    app.cleanup();
    let owner = app.world_mut().spawn_empty().id();
    let asset = app.world().resource::<AssetServer>().load("input.kui");
    let mount = app.world_mut().spawn(KuiView::new(owner, asset)).id();
    crate::tests::wait_for(&mut app, |app| {
        app.world().get::<KuiView>(mount).unwrap().status() == &KuiViewStatus::Ready
    });
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<Camera>>()
        .single(app.world())
        .unwrap();
    let root = app.world().get::<KuiView>(mount).unwrap().roots()[0];
    app.world_mut()
        .entity_mut(root)
        .insert(UiTargetCamera(camera));
    app.update();
    app.update();
    let input = |app: &mut App, position, action| {
        app.world_mut().write_message(PointerInput::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::None {
                    width: 800,
                    height: 600,
                },
                position,
            },
            action,
        ));
    };
    input(
        &mut app,
        Vec2::splat(10.),
        PointerAction::Move { delta: Vec2::ZERO },
    );
    app.update();
    assert_eq!(
        app.world()
            .resource::<Recorded>()
            .0
            .iter()
            .map(|e| e.action.as_str())
            .collect::<Vec<_>>(),
        ["enter", "hover"]
    );
    input(
        &mut app,
        Vec2::splat(10.),
        PointerAction::Press(PointerButton::Primary),
    );
    app.update();
    input(
        &mut app,
        Vec2::splat(20.),
        PointerAction::Move {
            delta: Vec2::splat(10.),
        },
    );
    app.update();
    input(
        &mut app,
        Vec2::splat(400.),
        PointerAction::Move {
            delta: Vec2::splat(380.),
        },
    );
    app.update();
    input(
        &mut app,
        Vec2::splat(400.),
        PointerAction::Release(PointerButton::Primary),
    );
    app.update();
    let events = &app.world().resource::<Recorded>().0;
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.action.as_str(), "start" | "move" | "end" | "cancel"))
            .map(|e| e.action.as_str())
            .collect::<Vec<_>>(),
        ["start", "move", "move", "end"]
    );
    assert!(
        !events
            .iter()
            .any(|e| e.action == "click" || e.action == "outer")
    );
    let start = events
        .iter()
        .find(|e| e.action == "start")
        .unwrap()
        .data
        .start
        .unwrap();
    let end = events
        .iter()
        .find(|e| e.action == "end")
        .unwrap()
        .data
        .position
        .unwrap();
    assert!((end - start).abs_diff_eq(Vec2::splat(390.), 0.001));
    assert_eq!(events.iter().filter(|e| e.action == "exit").count(), 1);
    // A full press/move/release sequence in one update still has one lifecycle.
    app.world_mut().resource_mut::<Recorded>().0.clear();
    input(
        &mut app,
        Vec2::splat(10.),
        PointerAction::Move {
            delta: Vec2::splat(-390.),
        },
    );
    app.update();
    input(
        &mut app,
        Vec2::splat(10.),
        PointerAction::Press(PointerButton::Primary),
    );
    input(
        &mut app,
        Vec2::splat(20.),
        PointerAction::Move {
            delta: Vec2::splat(10.),
        },
    );
    input(
        &mut app,
        Vec2::splat(20.),
        PointerAction::Release(PointerButton::Primary),
    );
    app.update();
    let events = &app.world().resource::<Recorded>().0;
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.action.as_str(), "start" | "move" | "end" | "cancel"))
            .map(|e| e.action.as_str())
            .collect::<Vec<_>>(),
        ["start", "move", "end"]
    );
    assert!(
        !events
            .iter()
            .any(|e| e.action == "click" || e.action == "outer")
    );
}

#[test]
fn component_event_forwarding_preserves_other_handlers_payload_and_geometry() {
    let view = format!(
        r#"{}<import name="item" src="item.kui"/><use component="item" on-press="inspect">
      <on name="chosen" action="choose"/><on name="tracked" action="track"/>
      </use></view>"#,
        crate::tests::HEADER
    );
    let component = r#"<component xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="item">
      <event name="chosen"/><event name="tracked"/>
      <node value="item" on-click="emit.chosen" on-hover="emit.tracked"><text id="geometry">Item</text></node>
      </component>"#;
    let mut app =
        crate::runtime::test_support::app(&[("view.kui", &view), ("item.kui", component)]);
    app.init_resource::<HoverMap>()
        .init_resource::<Recorded>()
        .add_observer(|event: On<KuiAction>, mut recorded: ResMut<Recorded>| {
            recorded.0.push(event.clone())
        });
    let owner = app.world_mut().spawn_empty().id();
    let asset = app.world().resource::<AssetServer>().load("view.kui");
    let mount = app
        .world_mut()
        .spawn(KuiView::new(owner, asset).in_world())
        .id();
    crate::tests::wait_for(&mut app, |app| {
        app.world().get::<KuiView>(mount).unwrap().status() == &KuiViewStatus::Ready
    });
    let geometry = named(&mut app, "geometry");
    set_hover(&mut app, geometry, Entity::PLACEHOLDER, Vec2::new(15., 25.));
    app.update();
    app.world_mut().trigger(PointerPress {
        entity: geometry,
        pointer: pointer(Vec2::ZERO),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 1., Some(Vec3::new(15., 25., 0.)), None),
        count: 1,
    });
    app.world_mut().flush();
    crate::tests::click(&mut app, geometry, PointerButton::Primary);
    let events = &app.world().resource::<Recorded>().0;
    assert_eq!(
        events.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(),
        ["track", "inspect", "choose"]
    );
    assert!(events.iter().all(|event| event.entity == owner
        && event.view == mount
        && event.value.as_deref() == Some("item")));
    assert_position(events[0].data.position, Vec2::new(15., 25.));
}

#[test]
fn layout_geometry_respects_viewport_dpi_ui_scale_and_parent_transform() {
    use bevy::ecs::system::RunSystemOnce;
    let (mut app, mount, target, _, camera) = fixture();
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .world_space = false;
    let frame = named(&mut app, "frame");
    app.world_mut().entity_mut(frame).insert((
        UiGlobalTransform::from(bevy::math::Affine2::from_scale_angle_translation(
            Vec2::splat(2.),
            std::f32::consts::FRAC_PI_2,
            Vec2::new(60., 40.),
        )),
        // Camera DPI 2 × UI scale 1.5.
        ComputedNode {
            inverse_scale_factor: 1. / 3.,
            ..default()
        },
    ));
    {
        let mut camera = app.world_mut().get_mut::<Camera>(camera).unwrap();
        camera.computed.target_info.as_mut().unwrap().scale_factor = 2.;
        camera.viewport = Some(bevy::camera::Viewport {
            physical_position: UVec2::new(100, 50),
            physical_size: UVec2::new(400, 300),
            ..default()
        });
    }
    let position = app
        .world_mut()
        .run_system_once(move |actions: Actions| {
            actions.position(target, Some(Vec2::new(100., 75.)), camera, None)
        })
        .unwrap();
    assert_position(position, Vec2::new(10., -20. / 3.));
}

#[test]
fn removing_hover_geometry_still_exits_on_the_surviving_controller() {
    let (mut app, _, target, source, camera) = fixture();
    set_hover(&mut app, source, camera, Vec2::new(10., 20.));
    app.update();
    app.world_mut().despawn(target);
    app.update();
    app.update();
    assert_eq!(
        app.world()
            .resource::<Recorded>()
            .0
            .iter()
            .map(|event| event.action.as_str())
            .collect::<Vec<_>>(),
        ["enter", "hover", "exit"]
    );
}
