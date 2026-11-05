use std::{collections::BTreeMap, time::Duration};

use bevy::{
    asset::AssetEvent,
    camera::NormalizedRenderTarget,
    picking::{
        backend::HitData,
        pointer::{Location, PointerId},
    },
    prelude::*,
    ui::InteractionDisabled,
};
use bevy_flair::prelude::{AttributeList, ClassList, StyleSheet, Styled};

use crate::{runtime::test_support, *};

pub(crate) const HEADER: &str = r#"<view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="test">"#;

#[test]
fn crate_documentation_renders_its_authored_examples() {
    use inslag_state_tree::{StateTreeDefinition, StateTreeInstance, StateTreePlugin, scxml};

    let docs = include_str!("lib.rs")
        .lines()
        .take_while(|line| line.starts_with("//!"))
        .map(|line| line.strip_prefix("//! ").unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let mut xml = docs.split("```xml\n").skip(1);
    let view = xml.next().unwrap().split("```").next().unwrap();
    let chart = xml.next().unwrap().split("```").next().unwrap();
    let css = docs
        .split("```css\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let definition = scxml::compile(&scxml::parse("ui/menu.scxml", chart).unwrap()).unwrap();
    let mut app = layout_app(&[("ui/menu.kui", view), ("ui/menu.css", css)]);
    app.add_plugins((
        bevy_flair::FlairPlugin,
        StateTreePlugin::new(Update),
        state_tree::KuiStateTreePlugin,
    ));
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition);
    app.world_mut().spawn(StateTreeInstance::new(handle));
    app.finish();
    app.cleanup();
    wait_for(&mut app, |app| {
        app.world_mut()
            .query::<&KuiView>()
            .iter(app.world())
            .any(|view| view.status() == &KuiViewStatus::Ready)
    });
    assert!(
        app.world_mut()
            .query::<&Text>()
            .iter(app.world())
            .any(|text| text.0 == "Coins: 1,250")
    );
}

#[test]
fn css_timelines_share_flair_imports_variables_and_sample_world_poses_without_lag_or_drift() {
    use bevy_flair::prelude::AnimationProgress;
    let source = format!(
        r#"{HEADER}<style src="motion.css"/><node>
      <node id="lid_a" class="lid" bind:attr-open="open">
        <node id="top_a" class="top" x="10"/>
        <node id="bottom_a" class="bottom"/>
      </node>
      <node id="lid_b" class="lid" bind:attr-open="true">
        <node id="top_b" class="top"/>
        <node id="bottom_b" class="bottom"/>
      </node>
    </node></view>"#
    );
    let mut app = layout_app(&[
        ("motion.kui", &source),
        (
            "motion.css",
            "@import './tracks.css'; .lid { --full: 1; -kui-timeline: --lid; -kui-progress: 0; transition: -kui-progress 1s linear; } .lid[open='true'] { -kui-progress: 1; } .top { -kui-animation-timeline: --lid; -kui-origin: 0 -100; animation: top 1s linear both; } .bottom { -kui-animation-timeline: --lid; animation: bottom 1s linear both; }",
        ),
        (
            "tracks.css",
            "@keyframes top { 0% { scale: var(--full) var(--full); } 50%, 100% { scale: 1 0; } } @keyframes bottom { 0%, 50% { scale: 1 0; } 100% { scale: var(--full) var(--full); } }",
        ),
    ]);
    app.add_message::<bevy::window::WindowEvent>()
        .add_plugins(bevy_flair::FlairPlugin)
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(100),
        ));
    app.finish();
    app.cleanup();
    let owner = app.world_mut().spawn_empty().id();
    let asset = app.world().resource::<AssetServer>().load("motion.kui");
    let mut view = KuiView::new(owner, asset).in_world();
    view.data.insert("open".into(), KuiValue::Boolean(false));
    let mount = app.world_mut().spawn(view).id();
    wait_for(&mut app, |app| {
        app.world_mut()
            .query::<&AnimationProgress>()
            .iter(app.world())
            .count()
            == 4
    });
    let top_a = named(&mut app, "top_a");
    let top_b = named(&mut app, "top_b");
    let bottom_a = named(&mut app, "bottom_a");
    let bottom_b = named(&mut app, "bottom_b");
    assert!(app.world().get::<Node>(top_a).is_none());
    assert_eq!(app.world().get::<Transform>(top_a).unwrap().scale.y, 1.0);
    assert_eq!(app.world().get::<Transform>(top_b).unwrap().scale.y, 0.0);
    assert_eq!(app.world().get::<Transform>(bottom_b).unwrap().scale.y, 1.0);
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("open".into(), KuiValue::Boolean(true));
    app.update(); // Establish the transition at its current pose.
    app.update();
    let progress = app.world().get::<AnimationProgress>(top_a).unwrap().get();
    assert!((progress - 0.1).abs() < 0.001, "{progress}");
    let transform = app.world().get::<Transform>(top_a).unwrap();
    assert!((transform.scale.y - 0.8).abs() < 0.001, "{transform:?}");
    assert!((transform.translation.y - 20.0).abs() < 0.001);
    assert_eq!(transform.translation.x, 10.0);
    assert_eq!(app.world().get::<Transform>(bottom_a).unwrap().scale.y, 0.0);
    assert_eq!(app.world().get::<Transform>(bottom_b).unwrap().scale.y, 1.0);
    // Reverse before completion: no jump to either authored endpoint.
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("open".into(), KuiValue::Boolean(false));
    app.update();
    let reversing = app.world().get::<Transform>(top_a).unwrap().scale.y;
    assert!((reversing - 0.6).abs() < 0.001, "{reversing}");
    for _ in 0..20 {
        app.update();
    }
    let transform = app.world().get::<Transform>(top_a).unwrap();
    assert_eq!(transform.scale, Vec3::ONE);
    assert_eq!(transform.translation, Vec3::new(10.0, 0.0, 0.0));
}

#[test]
fn css_timeline_components_are_scoped_in_both_contexts_and_reload_without_remounting() {
    use bevy::ecs::system::RunSystemOnce;
    use bevy_flair::{parser::InlineCssStyleSheetParser, prelude::AnimationProgress};
    let view = format!(
        r#"{HEADER}<import name="hinge" src="hinge.kui"/>
      <node><use component="hinge" id="first"><prop name="open" value="true"/></use>
      <use component="hinge" id="second"><prop name="open" value="false"/></use></node></view>"#
    );
    let component = r#"<component xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="hinge">
      <style src="hinge.css"/><prop name="open" type="boolean"/>
      <node class="hinge" bind:attr-open="props.open"><node class="part"/></node></component>"#;
    let css = ".hinge { -kui-timeline: --hinge; -kui-progress: 0; } .hinge[open='true'] { -kui-progress: 1; } .part { width: 100px; height: 40px; -kui-origin: 0 -20; -kui-animation-timeline: --hinge; animation: hinge 1s linear both; } @keyframes hinge { from { scale: 1 1; } to { scale: 1 0.5; } }";
    for world in [false, true] {
        let mut app = layout_app(&[
            ("view.kui", &view),
            ("hinge.kui", component),
            ("hinge.css", css),
        ]);
        app.add_message::<bevy::window::WindowEvent>()
            .add_plugins(bevy_flair::FlairPlugin);
        app.finish();
        app.cleanup();
        let owner = app.world_mut().spawn_empty().id();
        let asset = app.world().resource::<AssetServer>().load("view.kui");
        let mut view = KuiView::new(owner, asset);
        view.world_space = world;
        app.world_mut().spawn(view);
        wait_for(&mut app, |app| {
            app.world_mut()
                .query::<&AnimationProgress>()
                .iter(app.world())
                .count()
                == 2
        });
        let first = named(&mut app, "first");
        let second = named(&mut app, "second");
        let a = app.world().get::<Children>(first).unwrap()[0];
        let b = app.world().get::<Children>(second).unwrap()[0];
        assert_eq!(app.world().get::<UiTransform>(a).unwrap().scale.y, 0.5);
        assert_eq!(app.world().get::<UiTransform>(b).unwrap().scale.y, 1.0);
        if world {
            assert_eq!(app.world().get::<Transform>(a).unwrap().translation.y, 10.0);
        } else {
            assert_eq!(
                app.world().get::<UiTransform>(a).unwrap().translation.y,
                Val::Px(-10.0)
            );
        }
        // Replace the loaded CSS asset as the file watcher does. Keyframes are
        // recompiled by Flair; neither component nor its child is remounted.
        let changed = css.replace("scale: 1 0.5", "scale: 1 0.25");
        let compiled = app
            .world_mut()
            .run_system_once(move |parser: InlineCssStyleSheetParser| {
                parser.load_stylesheet(&changed).unwrap()
            })
            .unwrap();
        let style = app
            .world()
            .resource::<AssetServer>()
            .get_handle::<StyleSheet>("hinge.css")
            .unwrap();
        app.world_mut()
            .resource_mut::<Assets<StyleSheet>>()
            .insert(style.id(), compiled)
            .unwrap();
        wait_for(&mut app, |app| {
            app.world().get::<UiTransform>(a).unwrap().scale.y == 0.25
        });
        assert_eq!(app.world().get::<Children>(first).unwrap()[0], a);
        for _ in 0..3 {
            app.update();
        }
        if world {
            assert_eq!(app.world().get::<Transform>(a).unwrap().translation.y, 15.0);
        } else {
            assert_eq!(
                app.world().get::<UiTransform>(a).unwrap().translation.y,
                Val::Px(-15.0)
            );
        }
        // A component cannot accidentally bind to a same-named outer timeline.
        app.world_mut()
            .entity_mut(first)
            .insert(bevy_flair::prelude::InlineStyle::new(
                "-kui-timeline: --other;",
            ));
        wait_for(&mut app, |app| {
            app.world().get::<AnimationProgress>(a).unwrap().get() == 0.0
        });
        assert_eq!(app.world().get::<AnimationProgress>(b).unwrap().get(), 0.0);
    }
}

#[test]
fn world_mount_uses_shared_composition_bindings_and_non_button_actions() {
    let mut app = test_support::app(&[]);
    let handle = asset_with_images(
        &mut app,
        r#"<node id="root" x="10"><image id="cable" src="cable.png" on-click="connect" value="ethernet" bind:visible="shown" bind:disabled="locked" bind:x="offset"/><text id="label" bind:text="label">Cable</text></node>"#,
    );
    let owner = app.world_mut().spawn_empty().id();
    let mut view = KuiView::new(owner, handle).in_world();
    view.data = BTreeMap::from([
        ("shown".into(), KuiValue::Boolean(true)),
        ("locked".into(), KuiValue::Boolean(false)),
        ("offset".into(), KuiValue::Integer(25)),
        ("label".into(), KuiValue::String("Connected".into())),
    ]);
    let mount = app.world_mut().spawn(view).id();
    #[derive(Resource, Default)]
    struct Actions(Vec<KuiAction>);
    app.init_resource::<Actions>().add_observer(
        |event: On<KuiAction>, mut actions: ResMut<Actions>| actions.0.push(event.clone()),
    );
    app.update();
    let cable = named(&mut app, "cable");
    let label = named(&mut app, "label");
    assert!(app.world().get::<Sprite>(cable).is_some());
    assert!(app.world().get::<Node>(cable).is_none());
    assert_eq!(
        app.world().get::<Transform>(cable).unwrap().translation.x,
        25.0
    );
    assert_eq!(app.world().get::<Text2d>(label).unwrap().0, "Connected");
    click(&mut app, cable, PointerButton::Primary);
    assert_eq!(app.world().resource::<Actions>().0[0].entity, owner);
    assert_eq!(app.world().resource::<Actions>().0[0].action, "connect");
    let mut view = app.world_mut().get_mut::<KuiView>(mount).unwrap();
    view.data.insert("shown".into(), KuiValue::Boolean(false));
    view.data.insert("locked".into(), KuiValue::Boolean(true));
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(cable).unwrap(),
        Visibility::Hidden
    );
    click(&mut app, cable, PointerButton::Primary);
    assert_eq!(app.world().resource::<Actions>().0.len(), 1);
    app.world_mut().despawn(owner);
    app.update();
    assert!(app.world().get_entity(cable).is_err());
}

#[test]
fn layout_images_and_containers_can_dispatch_actions() {
    let mut app = test_support::app(&[]);
    let handle = asset_with_images(
        &mut app,
        r#"<node id="group" on-click="group"><image id="image" src="image.png" on-click="image"/></node>"#,
    );
    let owner = app.world_mut().spawn_empty().id();
    app.world_mut().spawn(KuiView::new(owner, handle));
    #[derive(Resource, Default)]
    struct Actions(Vec<String>);
    app.init_resource::<Actions>().add_observer(
        |event: On<KuiAction>, mut actions: ResMut<Actions>| actions.0.push(event.action.clone()),
    );
    app.update();
    let image = named(&mut app, "image");
    let group = named(&mut app, "group");
    click(&mut app, image, PointerButton::Primary);
    click(&mut app, group, PointerButton::Primary);
    assert_eq!(app.world().resource::<Actions>().0, ["image", "group"]);
}

fn item(id: KuiValue, label: &str) -> KuiValue {
    KuiValue::Record(BTreeMap::from([
        ("id".into(), id),
        ("label".into(), KuiValue::String(label.into())),
    ]))
}

#[test]
fn luau_compound_transforms_update_atomically_and_recover() {
    let mut app = test_support::app(&[]);
    let handle = asset(&mut app, "<node/>");
    let source = format!(
        r#"{HEADER}<node id="pose" bind:position="pose" bind:scale="if enlarged then {{x=2,y=3}} else {{x=1,y=1}}"/></view>"#
    );
    app.world_mut()
        .resource_mut::<Assets<KuiAsset>>()
        .get_mut(&handle)
        .unwrap()
        .definition = parse("expression.kui", &source).unwrap();
    let owner = app.world_mut().spawn_empty().id();
    let position = |y| {
        KuiValue::Record(BTreeMap::from([
            ("x".into(), KuiValue::Integer(5)),
            ("y".into(), y),
            ("z".into(), KuiValue::Integer(8)),
        ]))
    };
    let mut view = KuiView::new(owner, handle).in_world();
    view.data
        .insert("pose".into(), position(KuiValue::Integer(10)));
    view.data.insert("enlarged".into(), KuiValue::Boolean(true));
    let mount = app.world_mut().spawn(view).id();
    app.update();
    app.update(); // Settle the synthetic asset's modification notification.
    let entity = named(&mut app, "pose");
    let transform = app.world().get::<Transform>(entity).unwrap();
    assert_eq!(transform.translation, Vec3::new(5.0, 10.0, 8.0));
    assert_eq!(transform.scale, Vec3::new(2.0, 3.0, 1.0));
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("pose".into(), position(KuiValue::String("invalid".into())));
    app.update();
    assert_eq!(
        app.world().get::<Transform>(entity).unwrap().translation,
        Vec3::new(5.0, 10.0, 8.0)
    );
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("pose".into(), position(KuiValue::Integer(25)));
    app.update();
    assert_eq!(
        app.world().get::<Transform>(entity).unwrap().translation,
        Vec3::new(5.0, 25.0, 8.0)
    );
}

#[test]
fn world_rotation_bindings_use_finite_radians_and_recover_without_remounting() {
    let mut app = test_support::app(&[]);
    let handle = asset(
        &mut app,
        r#"<node id="rotated" x="5" scale-y="3" bind:rotation="angle"/>"#,
    );
    let owner = app.world_mut().spawn_empty().id();
    let mut view = KuiView::new(owner, handle).in_world();
    view.data.insert(
        "angle".into(),
        KuiValue::Number(std::f64::consts::FRAC_PI_2),
    );
    let mount = app.world_mut().spawn(view).id();
    app.update();
    app.update();
    let entity = named(&mut app, "rotated");
    let expected = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let transform = app.world().get::<Transform>(entity).unwrap();
    assert!(transform.rotation.abs_diff_eq(expected, 0.0001));
    assert_eq!(transform.translation, Vec3::new(5., 0., 0.));
    assert_eq!(transform.scale, Vec3::new(1., 3., 1.));
    for invalid in [
        KuiValue::Number(f64::NAN),
        KuiValue::Number(f64::INFINITY),
        KuiValue::Number(f64::MAX),
        KuiValue::String("invalid".into()),
        KuiValue::Nil,
    ] {
        app.world_mut()
            .get_mut::<KuiView>(mount)
            .unwrap()
            .data
            .insert("angle".into(), invalid);
        app.update();
        assert!(
            app.world()
                .get::<Transform>(entity)
                .unwrap()
                .rotation
                .abs_diff_eq(expected, 0.0001)
        );
    }
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("angle".into(), KuiValue::Integer(-1));
    app.update();
    assert_eq!(named(&mut app, "rotated"), entity);
    assert!(
        app.world()
            .get::<Transform>(entity)
            .unwrap()
            .rotation
            .abs_diff_eq(Quat::from_rotation_z(-1.), 0.0001)
    );
}

#[test]
fn rotation_bindings_require_a_world_presentation_context() {
    let mut app = test_support::app(&[]);
    let handle = asset(&mut app, r#"<node bind:rotation="0.5"/>"#);
    let owner = app.world_mut().spawn_empty().id();
    let mount = app.world_mut().spawn(KuiView::new(owner, handle)).id();
    app.update();
    assert!(matches!(
        app.world().get::<KuiView>(mount).unwrap().status(),
        KuiViewStatus::Failed(message) if message.contains("world presentation context")
    ));
}

#[test]
fn world_rotation_bindings_compose_with_css_without_accumulating() {
    let source = format!(
        r#"{HEADER}<style src="rotation.css"/><node id="rotated" class="rotated" bind:rotation="angle"/></view>"#
    );
    let mut app = layout_app(&[
        ("rotation.kui", &source),
        ("rotation.css", ".rotated { rotate: 30deg; }"),
    ]);
    app.add_message::<bevy::window::WindowEvent>()
        .add_plugins(bevy_flair::FlairPlugin);
    app.finish();
    app.cleanup();
    let owner = app.world_mut().spawn_empty().id();
    let handle = app.world().resource::<AssetServer>().load("rotation.kui");
    let mut view = KuiView::new(owner, handle).in_world();
    view.data.insert("angle".into(), KuiValue::Number(1.));
    let mount = app.world_mut().spawn(view).id();
    wait_for(&mut app, |app| {
        app.world_mut()
            .query::<(&Name, &Transform)>()
            .iter(app.world())
            .any(|(name, transform)| {
                name.as_str() == "rotated"
                    && transform.rotation.abs_diff_eq(
                        Quat::from_rotation_z(1. - std::f32::consts::PI / 6.),
                        0.0001,
                    )
            })
    });
    let entity = named(&mut app, "rotated");
    for angle in [2., -0.5] {
        app.world_mut()
            .get_mut::<KuiView>(mount)
            .unwrap()
            .data
            .insert("angle".into(), KuiValue::Number(angle));
        for _ in 0..4 {
            app.update();
            assert!(
                app.world()
                    .get::<Transform>(entity)
                    .unwrap()
                    .rotation
                    .abs_diff_eq(
                        Quat::from_rotation_z(angle as f32 - std::f32::consts::PI / 6.),
                        0.0001,
                    )
            );
        }
    }
}

#[test]
fn luau_bindings_and_compound_conflicts_are_explicit() {
    let source = |body: &str| format!("{HEADER}{body}</view>");
    assert!(parse("expression.kui", &source(r#"<text bind:text="1 + 2"/>"#)).is_ok());
    for body in [
        r#"<node bind:position="pose" bind:x="x"/>"#,
        r#"<node bind:position="pose" z="3"/>"#,
        r#"<node bind:scale="scale" bind:scale-x="x"/>"#,
    ] {
        assert!(parse("conflict.kui", &source(body)).is_err());
    }
}

#[test]
fn luau_repeats_components_and_slots_keep_lexical_scopes_after_reordering() {
    let source = format!(
        r#"{HEADER}<import name="card" src="card.kui"/><node><repeat bind:items="if enabled then rows else {{}}" as="row" key="id * 10"><use component="card"><prop name="item" bind:value="row"/><fill slot="detail"><text template="{{prefix}}: {{row.label}}"/></fill></use></repeat></node></view>"#
    );
    let component = r#"<component xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="card"><prop name="item" type="record"/><node><text bind:text="string.upper(props.item.label)"/><text>isolated</text><slot name="detail"/></node></component>"#;
    let mut app = test_support::app(&[("expressions.kui", &source), ("card.kui", component)]);
    let owner = app.world_mut().spawn_empty().id();
    let asset = app
        .world()
        .resource::<AssetServer>()
        .load("expressions.kui");
    let mut view = KuiView::new(owner, asset);
    let rows = |reverse| {
        let mut rows = vec![
            item(KuiValue::Integer(1), "Oak"),
            item(KuiValue::Integer(2), "Pine"),
        ];
        if reverse {
            rows.reverse();
        }
        KuiValue::Sequence(rows)
    };
    view.data = BTreeMap::from([
        ("enabled".into(), KuiValue::Boolean(true)),
        ("prefix".into(), KuiValue::String("Tree".into())),
        ("rows".into(), rows(false)),
    ]);
    let mount = app.world_mut().spawn(view).id();
    wait_for(&mut app, |app| repeated(app).len() == 2);
    let initial = repeated(&mut app);
    assert!(initial.contains_key(&KuiItemKey::Integer(10)));
    let texts = |app: &mut App| {
        app.world_mut()
            .query::<&Text>()
            .iter(app.world())
            .map(|t| t.0.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        texts(&mut app),
        [
            "OAK",
            "isolated",
            "Tree: Oak",
            "PINE",
            "isolated",
            "Tree: Pine"
        ]
    );
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("rows".into(), rows(true));
    app.update();
    assert_eq!(repeated(&mut app), initial);
    assert!(texts(&mut app).contains(&"Tree: Pine".to_owned()));
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("enabled".into(), KuiValue::Boolean(false));
    app.update();
    assert!(repeated(&mut app).is_empty());
}

#[test]
fn keyed_lookups_follow_repeat_aliases_retarget_reorder_and_recover_missing_values() {
    let mut app = test_support::app(&[]);
    let handle = asset(
        &mut app,
        r#"<node><repeat bind:items="rows" as="row" key="id"><node><text bind:text="values[row.label]">Missing</text><text template="Value {values[row.label]:fixed.1}"/></node></repeat></node>"#,
    );
    let owner = app.world_mut().spawn_empty().id();
    let mut view = KuiView::new(owner, handle);
    view.data = BTreeMap::from([
        (
            "rows".into(),
            KuiValue::Sequence(vec![
                item(KuiValue::Integer(1), "a.b[0]"),
                item(KuiValue::Integer(2), "other"),
            ]),
        ),
        (
            "values".into(),
            KuiValue::Record(BTreeMap::from([
                ("a.b[0]".into(), KuiValue::Integer(7)),
                ("other".into(), KuiValue::Integer(9)),
            ])),
        ),
    ]);
    let mount = app.world_mut().spawn(view).id();
    app.update();
    let initial = repeated(&mut app);
    let row = initial[&KuiItemKey::Integer(1)];
    let children = app.world().get::<Children>(row).unwrap().to_vec();
    assert_eq!(app.world().get::<Text>(children[0]).unwrap().0, "7");
    assert_eq!(app.world().get::<Text>(children[1]).unwrap().0, "Value 7.0");
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert(
            "rows".into(),
            KuiValue::Sequence(vec![
                item(KuiValue::Integer(2), "other"),
                item(KuiValue::Integer(1), "other"),
            ]),
        );
    app.update();
    assert_eq!(repeated(&mut app), initial);
    assert_eq!(app.world().get::<Text>(children[0]).unwrap().0, "9");
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("values".into(), KuiValue::Record(BTreeMap::new()));
    app.update();
    // Missing lookups follow existing text binding semantics: retain the last valid text.
    assert_eq!(app.world().get::<Text>(children[0]).unwrap().0, "9");
    assert_eq!(app.world().get::<Text>(children[1]).unwrap().0, "Value 9.0");
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert(
            "values".into(),
            KuiValue::Record(BTreeMap::from([("other".into(), KuiValue::Integer(11))])),
        );
    app.update();
    assert_eq!(app.world().get::<Text>(children[0]).unwrap().0, "11");
}

fn repeated(app: &mut App) -> BTreeMap<KuiItemKey, Entity> {
    app.world_mut()
        .query::<(Entity, &KuiRepeatItem)>()
        .iter(app.world())
        .map(|(entity, row)| (row.key.clone(), entity))
        .collect()
}

#[test]
fn keyed_rows_preserve_entities_reorder_update_and_remove_without_layout_wrappers() {
    let mut app = test_support::app(&[]);
    let handle = asset(
        &mut app,
        r#"<node id="list"><text id="before">Before</text><repeat bind:items="items" as="item" key="id"><button on-click="select" bind:value="item.id"><text bind:text="item.label"/></button></repeat><text id="after">After</text></node>"#,
    );
    let owner = app.world_mut().spawn_empty().id();
    let mut view = KuiView::new(owner, handle);
    view.data.insert(
        "items".into(),
        KuiValue::Sequence(vec![
            item(KuiValue::Integer(1), "First"),
            item(KuiValue::String("1".into()), "Second"),
        ]),
    );
    let mount = app.world_mut().spawn(view).id();
    app.update();
    let initial = repeated(&mut app);
    assert_eq!(initial.len(), 2);
    let a = initial[&KuiItemKey::Integer(1)];
    let b = initial[&KuiItemKey::String("1".into())];
    let list = named(&mut app, "list");
    let before = named(&mut app, "before");
    let after = named(&mut app, "after");
    assert_eq!(
        app.world().get::<Children>(list).unwrap().to_vec(),
        [before, a, b, after]
    );
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert(
            "items".into(),
            KuiValue::Sequence(vec![
                item(KuiValue::String("1".into()), "Changed"),
                item(KuiValue::Integer(1), "First"),
            ]),
        );
    app.update();
    assert_eq!(repeated(&mut app), initial);
    assert_eq!(
        app.world().get::<Children>(list).unwrap().to_vec(),
        [before, b, a, after]
    );
    let text = app.world().get::<Children>(b).unwrap()[0];
    assert_eq!(app.world().get::<Text>(text).unwrap().0, "Changed");
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert(
            "items".into(),
            KuiValue::Sequence(vec![
                item(KuiValue::Integer(1), "Retained"),
                item(KuiValue::Integer(2), "New"),
            ]),
        );
    app.update();
    assert!(app.world().get_entity(b).is_err());
    assert_eq!(repeated(&mut app)[&KuiItemKey::Integer(1)], a);
    assert_eq!(repeated(&mut app).len(), 2);
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("items".into(), KuiValue::Sequence(vec![]));
    app.update();
    assert!(repeated(&mut app).is_empty());
    assert_eq!(
        app.world().get::<Children>(list).unwrap().to_vec(),
        [before, after]
    );
}

#[test]
fn invalid_collections_clear_rows_atomically_and_recover() {
    let mut app = test_support::app(&[]);
    let handle = asset(
        &mut app,
        r#"<node><repeat bind:items="items" as="row" key="id"><text bind:text="row.label"/></repeat></node>"#,
    );
    let owner = app.world_mut().spawn_empty().id();
    let mut view = KuiView::new(owner, handle);
    view.data.insert(
        "items".into(),
        KuiValue::Sequence(vec![item(KuiValue::Integer(1), "A")]),
    );
    let mount = app.world_mut().spawn(view).id();
    app.update();
    for invalid in [
        KuiValue::Nil,
        KuiValue::Sequence(vec![
            item(KuiValue::Integer(1), "A"),
            item(KuiValue::Integer(1), "B"),
        ]),
        KuiValue::Sequence(vec![item(KuiValue::Nil, "Missing")]),
        KuiValue::Sequence(vec![KuiValue::Boolean(true)]),
    ] {
        app.world_mut()
            .get_mut::<KuiView>(mount)
            .unwrap()
            .data
            .insert("items".into(), invalid);
        app.update();
        assert!(repeated(&mut app).is_empty());
        app.world_mut()
            .get_mut::<KuiView>(mount)
            .unwrap()
            .data
            .insert(
                "items".into(),
                KuiValue::Sequence(vec![item(KuiValue::Integer(2), "Recovered")]),
            );
        app.update();
        assert_eq!(repeated(&mut app).len(), 1);
    }
}

#[test]
fn nested_repeats_resolve_outer_aliases_and_owner_data() {
    let mut app = test_support::app(&[]);
    let handle = asset(
        &mut app,
        r#"<node><repeat bind:items="groups" as="group" key="id"><node><repeat bind:items="group.items" as="item" key="id"><node><text bind:text="group.label"/><text bind:text="item.label"/><text bind:text="title"/></node></repeat></node></repeat></node>"#,
    );
    let owner = app.world_mut().spawn_empty().id();
    let mut group = match item(KuiValue::Integer(1), "Outer") {
        KuiValue::Record(value) => value,
        _ => unreachable!(),
    };
    group.insert(
        "items".into(),
        KuiValue::Sequence(vec![item(KuiValue::Integer(2), "Inner")]),
    );
    let mut view = KuiView::new(owner, handle);
    view.data = BTreeMap::from([
        (
            "groups".into(),
            KuiValue::Sequence(vec![KuiValue::Record(group)]),
        ),
        ("title".into(), KuiValue::String("Owner".into())),
    ]);
    let mount = app.world_mut().spawn(view).id();
    app.update();
    let texts = app
        .world_mut()
        .query::<&Text>()
        .iter(app.world())
        .map(|text| text.0.clone())
        .collect::<Vec<_>>();
    assert_eq!(texts, ["Outer", "Inner", "Owner"]);
    let mut view = app.world_mut().get_mut::<KuiView>(mount).unwrap();
    let KuiValue::Sequence(groups) = view.data.get_mut("groups").unwrap() else {
        unreachable!()
    };
    let KuiValue::Record(group) = &mut groups[0] else {
        unreachable!()
    };
    group.insert("label".into(), KuiValue::String("Changed outer".into()));
    app.update();
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("title".into(), KuiValue::String("Changed owner".into()));
    app.update();
    let texts = app
        .world_mut()
        .query::<&Text>()
        .iter(app.world())
        .map(|text| text.0.as_str())
        .collect::<Vec<_>>();
    assert_eq!(texts, ["Changed outer", "Inner", "Changed owner"]);
}

#[test]
fn parser_enforces_repeat_and_dynamic_value_contracts() {
    for body in [
        r#"<text bind:text="value" template="{value}"/>"#,
        r#"<text format="grouped"/>"#,
        r#"<text template="{value}" format="grouped"/>"#,
        r#"<node template="{value}"/>"#,
        r#"<repeat bind:items="items" as="row" key="id"><node/></repeat>"#,
        r#"<node><repeat bind:items="items" as="row"><node/></repeat></node>"#,
        r#"<node><repeat bind:items="items" as="row" key="id"><node/><node/></repeat></node>"#,
        r#"<node><repeat bind:items="items" as="row.bad" key="id"><node/></repeat></node>"#,
        r#"<button on-click="x" value="x" bind:value="value"/>"#,
        r#"<button on-click="x"><repeat bind:items="items" as="row" key="id"><button on-click="y"/></repeat></button>"#,
    ] {
        assert!(
            parse("synthetic", &format!("{HEADER}{body}</view>")).is_err(),
            "{body}"
        );
    }
    assert!(parse("synthetic", &format!(r#"{HEADER}<node><repeat bind:items="a" as="row" key="id"><text id="label"/></repeat><repeat bind:items="b" as="row" key="id"><text id="label"/></repeat></node></view>"#)).is_ok());
}

fn asset(app: &mut App, body: &str) -> Handle<KuiAsset> {
    let definition = parse("synthetic.kui", &format!("{HEADER}{body}</view>")).unwrap();
    app.world_mut()
        .resource_mut::<Assets<KuiAsset>>()
        .add(KuiAsset {
            component_style_paths: vec![],
            component_style_handles: vec![],
            definition,
            style_sheet: None,
            image_paths: vec![],
            image_handles: vec![],
        })
}

fn asset_with_images(app: &mut App, body: &str) -> Handle<KuiAsset> {
    let definition = parse("synthetic.kui", &format!("{HEADER}{body}</view>")).unwrap();
    let mut image_paths = Vec::new();
    crate::asset::collect_image_paths(&definition.children, &mut image_paths);
    let image_handles = image_paths.iter().map(|_| Handle::default()).collect();
    app.world_mut()
        .resource_mut::<Assets<KuiAsset>>()
        .add(KuiAsset {
            definition,
            image_paths,
            image_handles,
            style_sheet: None,
            component_style_paths: vec![],
            component_style_handles: vec![],
        })
}

fn named(app: &mut App, name: &str) -> Entity {
    app.world_mut()
        .query::<(Entity, &Name)>()
        .iter(app.world())
        .find_map(|(entity, candidate)| (candidate.as_str() == name).then_some(entity))
        .unwrap()
}

#[test]
fn scalar_text_and_templates_format_update_atomically_and_recover() {
    let mut app = test_support::app(&[]);
    let handle = asset(
        &mut app,
        r#"<node><text id="scalar" bind:text="amount" format="grouped"/><text id="boolean" bind:text="enabled"/><text id="summary" template="Health {amount:grouped} / {maximum:grouped} · {progress:percent.1}"/></node>"#,
    );
    let owner = app.world_mut().spawn_empty().id();
    let mut view = KuiView::new(owner, handle);
    view.data = BTreeMap::from([
        ("amount".into(), KuiValue::Integer(1234)),
        ("maximum".into(), KuiValue::Integer(2000)),
        ("progress".into(), KuiValue::Number(0.625)),
        ("enabled".into(), KuiValue::Boolean(true)),
    ]);
    let mount = app.world_mut().spawn(view).id();
    app.update();
    let scalar = named(&mut app, "scalar");
    let summary = named(&mut app, "summary");
    let boolean = named(&mut app, "boolean");
    assert_eq!(app.world().get::<Text>(scalar).unwrap().0, "1,234");
    assert_eq!(app.world().get::<Text>(boolean).unwrap().0, "true");
    assert_eq!(
        app.world().get::<Text>(summary).unwrap().0,
        "Health 1,234 / 2,000 · 62.5%"
    );
    app.world_mut().clear_trackers();
    app.update();
    assert!(
        !app.world()
            .entity(summary)
            .get_ref::<Text>()
            .unwrap()
            .is_changed()
    );
    {
        let mut view = app.world_mut().get_mut::<KuiView>(mount).unwrap();
        view.data.insert("amount".into(), KuiValue::Integer(0));
        view.data
            .insert("progress".into(), KuiValue::Number(f64::INFINITY));
    }
    app.update();
    assert_eq!(app.world().get::<Text>(scalar).unwrap().0, "0");
    assert_eq!(
        app.world().get::<Text>(summary).unwrap().0,
        "Health 1,234 / 2,000 · 62.5%"
    );
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("progress".into(), KuiValue::Number(0.0));
    app.update();
    assert_eq!(
        app.world().get::<Text>(summary).unwrap().0,
        "Health 0 / 2,000 · 0.0%"
    );
}

#[test]
fn text_templates_follow_keyed_rows_after_reordering() {
    let mut app = test_support::app(&[]);
    let handle = asset(
        &mut app,
        r#"<node><repeat bind:items="items" as="item" key="id"><text template="{item.label} · {item.id} / {limit}"/></repeat></node>"#,
    );
    let owner = app.world_mut().spawn_empty().id();
    let mut view = KuiView::new(owner, handle);
    view.data.insert("limit".into(), KuiValue::Integer(10));
    view.data.insert(
        "items".into(),
        KuiValue::Sequence(vec![
            item(KuiValue::Integer(1), "First"),
            item(KuiValue::Integer(2), "Second"),
        ]),
    );
    let mount = app.world_mut().spawn(view).id();
    app.update();
    let rows = repeated(&mut app);
    let first = rows[&KuiItemKey::Integer(1)];
    assert_eq!(app.world().get::<Text>(first).unwrap().0, "First · 1 / 10");
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert(
            "items".into(),
            KuiValue::Sequence(vec![
                item(KuiValue::Integer(2), "Second"),
                item(KuiValue::Integer(1), "Changed"),
            ]),
        );
    app.update();
    assert_eq!(repeated(&mut app), rows);
    assert_eq!(
        app.world().get::<Text>(first).unwrap().0,
        "Changed · 1 / 10"
    );
}

pub(crate) fn wait_for(app: &mut App, predicate: impl Fn(&mut App) -> bool) {
    for _ in 0..1000 {
        app.update();
        if predicate(app) {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("observable condition did not become true");
}

pub(crate) fn click(app: &mut App, entity: Entity, button: PointerButton) {
    app.world_mut().trigger(PointerClick {
        entity,
        pointer: Pointer::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::None {
                    width: 0,
                    height: 0,
                },
                position: Vec2::ZERO,
            },
        ),
        button,
        hit: HitData {
            camera: Entity::PLACEHOLDER,
            depth: 0.0,
            position: None,
            normal: None,
            extra: None,
        },
        duration: Duration::ZERO,
        count: 1,
    });
    app.world_mut().flush();
}

#[test]
fn standalone_mount_applies_hierarchy_styles_images_and_typed_bindings() {
    let mut app = test_support::app(&[]);
    let handle = asset(
        &mut app,
        r#"<node id="root" class="panel" bind:visible="ui.open" bind:attr-count="ui.count"><text id="label" bind:text="ui.label">waiting</text><image id="icon" src="icon.png"/><button id="button" on-click="choose" bind:disabled="ui.disabled"><text>Choose</text></button></node>"#,
    );
    let image = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::default());
    let style = Handle::<StyleSheet>::default();
    {
        let mut assets = app.world_mut().resource_mut::<Assets<KuiAsset>>();
        let mut asset = assets.get_mut(&handle).unwrap();
        asset.image_paths.push("icon.png".into());
        asset.image_handles.push(image.clone());
        asset.style_sheet = Some(style);
    }
    // Publish the asset edit before mounting; it is not a hot reload of this view.
    app.update();
    let owner = app.world_mut().spawn_empty().id();
    let mut view = KuiView::new(owner, handle);
    view.data.insert(
        "ui".into(),
        KuiValue::Record(BTreeMap::from([
            ("open".into(), KuiValue::Boolean(false)),
            ("label".into(), KuiValue::String("ready".into())),
            ("count".into(), KuiValue::Integer(7)),
            ("disabled".into(), KuiValue::Boolean(false)),
        ])),
    );
    let mount = app.world_mut().spawn(view).id();
    app.update();
    let root = named(&mut app, "root");
    let label = named(&mut app, "label");
    let icon = named(&mut app, "icon");
    let button = named(&mut app, "button");
    assert_eq!(
        app.world().get::<KuiView>(mount).unwrap().status(),
        &KuiViewStatus::Ready
    );
    assert_eq!(app.world().get::<KuiView>(mount).unwrap().roots(), &[root]);
    assert_eq!(
        app.world().get::<KuiViewRoot>(root).unwrap().controller,
        owner
    );
    assert!(app.world().get::<ClassList>(root).is_some());
    assert!(app.world().get::<Styled>(root).is_some());
    assert_eq!(app.world().get::<Text>(label).unwrap().0, "ready");
    assert_eq!(
        app.world().get::<Node>(root).unwrap().display,
        Display::None
    );
    assert_eq!(
        app.world()
            .get::<AttributeList>(root)
            .unwrap()
            .get_attribute("count"),
        Some("7")
    );
    assert_eq!(app.world().get::<ImageNode>(icon).unwrap().image, image);
    assert_eq!(app.world().get::<ChildOf>(label).unwrap().parent(), root);
    assert_eq!(app.world().get::<Pickable>(label), Some(&Pickable::IGNORE));
    assert!(app.world().get::<InteractionDisabled>(button).is_none());

    app.world_mut().clear_trackers();
    app.update();
    assert!(
        !app.world()
            .entity(label)
            .get_ref::<Text>()
            .unwrap()
            .is_changed()
    );

    {
        let mut view = app.world_mut().get_mut::<KuiView>(mount).unwrap();
        let KuiValue::Record(data) = view.data.get_mut("ui").unwrap() else {
            panic!()
        };
        data.insert("count".into(), KuiValue::Nil);
        data.insert("label".into(), KuiValue::Integer(99));
        data.remove("disabled");
    }
    app.update();
    assert_eq!(
        app.world()
            .get::<AttributeList>(root)
            .unwrap()
            .get_attribute("count"),
        None
    );
    assert_eq!(app.world().get::<Text>(label).unwrap().0, "99");
    assert!(app.world().get::<InteractionDisabled>(button).is_some());
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert(
            "ui".into(),
            KuiValue::Record(BTreeMap::from([(
                "disabled".into(),
                KuiValue::Boolean(false),
            )])),
        );
    app.update();
    assert!(app.world().get::<InteractionDisabled>(button).is_none());
}

#[derive(Resource, Default)]
struct Actions(Vec<KuiAction>);

#[test]
fn dynamic_values_dispatch_current_payload_and_fail_closed_until_valid_again() {
    let mut app = test_support::app(&[]);
    app.init_resource::<Actions>().add_observer(
        |action: On<KuiAction>, mut actions: ResMut<Actions>| actions.0.push(action.clone()),
    );
    let handle = asset(
        &mut app,
        r#"<button id="dynamic" on-click="select" bind:value="value"/>"#,
    );
    let owner = app.world_mut().spawn_empty().id();
    let mount = app.world_mut().spawn(KuiView::new(owner, handle)).id();
    app.update();
    let button = named(&mut app, "dynamic");
    click(&mut app, button, PointerButton::Primary);
    assert!(app.world().resource::<Actions>().0.is_empty());
    for value in [
        KuiValue::String("A".into()),
        KuiValue::Sequence(vec![]),
        KuiValue::String("A".into()),
        KuiValue::Integer(7),
        KuiValue::Nil,
    ] {
        app.world_mut()
            .get_mut::<KuiView>(mount)
            .unwrap()
            .data
            .insert("value".into(), value);
        app.update();
        click(&mut app, button, PointerButton::Primary);
    }
    let payloads = app
        .world()
        .resource::<Actions>()
        .0
        .iter()
        .map(|action| action.value.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(payloads, [Some("A"), Some("A"), Some("7"), None]);
}

#[test]
fn actions_are_owner_targeted_and_filter_disabled_and_secondary_clicks() {
    let mut app = test_support::app(&[]);
    app.init_resource::<Actions>().add_observer(
        |action: On<KuiAction>, mut actions: ResMut<Actions>| {
            actions.0.push(action.event().clone())
        },
    );
    let handle = asset(
        &mut app,
        r#"<button id="choice" on-click="choose" value="oak" bind:disabled="disabled"><text>Choose</text></button>"#,
    );
    let owner = app.world_mut().spawn_empty().id();
    let mut view = KuiView::new(owner, handle);
    view.data.insert("disabled".into(), KuiValue::Boolean(true));
    let mount = app.world_mut().spawn(view).id();
    app.update();
    let button = named(&mut app, "choice");
    click(&mut app, button, PointerButton::Primary);
    assert!(app.world().resource::<Actions>().0.is_empty());
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("disabled".into(), KuiValue::Boolean(false));
    app.update();
    click(&mut app, button, PointerButton::Secondary);
    assert!(app.world().resource::<Actions>().0.is_empty());
    click(&mut app, button, PointerButton::Primary);
    let actions = &app.world().resource::<Actions>().0;
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].entity, owner);
    assert_eq!(actions[0].view, mount);
    assert_eq!(actions[0].action, "choose");
    assert_eq!(actions[0].value.as_deref(), Some("oak"));
}

#[test]
fn reload_rebuilds_roots_and_bindings_and_mount_removal_cleans_children() {
    let mut app = test_support::app(&[]);
    let handle = asset(
        &mut app,
        r#"<node><text bind:text="label">before</text></node>"#,
    );
    let owner = app.world_mut().spawn_empty().id();
    let mut view = KuiView::new(owner, handle.clone());
    view.data
        .insert("label".into(), KuiValue::String("initial".into()));
    let mount = app.world_mut().spawn(view).id();
    app.update();
    let old_root = app.world().get::<KuiView>(mount).unwrap().roots()[0];
    app.world_mut()
        .resource_mut::<Assets<KuiAsset>>()
        .get_mut(&handle)
        .unwrap()
        .definition = parse(
        "reload.kui",
        &format!("{HEADER}<text bind:text=\"label\">after</text></view>"),
    )
    .unwrap();
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("label".into(), KuiValue::String("reloaded".into()));
    app.world_mut()
        .write_message(AssetEvent::<KuiAsset>::Modified { id: handle.id() });
    app.update();
    assert!(app.world().get_entity(old_root).is_err());
    assert_eq!(
        app.world_mut()
            .query::<&Text>()
            .single(app.world())
            .unwrap()
            .0,
        "reloaded"
    );
    assert_eq!(app.world().get::<KuiView>(mount).unwrap().roots().len(), 1);
    app.world_mut().despawn(mount);
    app.update();
    assert_eq!(
        app.world_mut().query::<&Text>().iter(app.world()).count(),
        0
    );
}

#[test]
fn removing_owner_or_asset_cleans_roots() {
    let mut app = test_support::app(&[]);
    let handle = asset(&mut app, "<text>hello</text>");
    let owner = app.world_mut().spawn_empty().id();
    let mount = app
        .world_mut()
        .spawn(KuiView::new(owner, handle.clone()))
        .id();
    app.update();
    app.world_mut()
        .resource_mut::<Assets<KuiAsset>>()
        .remove(handle.id());
    app.world_mut()
        .write_message(AssetEvent::<KuiAsset>::Removed { id: handle.id() });
    app.update();
    assert_eq!(
        app.world_mut().query::<&Text>().iter(app.world()).count(),
        0
    );
    assert_eq!(
        app.world().get::<KuiView>(mount).unwrap().status(),
        &KuiViewStatus::Loading
    );
    app.world_mut().despawn(owner);
    app.update();
    assert!(app.world().get_entity(mount).is_err());
}

#[test]
fn loader_tracks_dependencies_and_reports_view_image_and_style_failures() {
    for body in [
        "<node/>",
        "<image src=\"missing.png\"/>",
        "<style src=\"missing.css\"/><node/>",
    ] {
        let source = format!("{HEADER}{body}</view>");
        let mut app = test_support::app(&[("test.kui", &source)]);
        let owner = app.world_mut().spawn_empty().id();
        let path = if body == "<node/>" {
            "missing.kui"
        } else {
            "test.kui"
        };
        let handle = app.world().resource::<AssetServer>().load::<KuiAsset>(path);
        let mount = app
            .world_mut()
            .spawn(KuiView::new(owner, handle.clone()))
            .id();
        wait_for(&mut app, |app| {
            matches!(
                app.world().get::<KuiView>(mount).unwrap().status(),
                KuiViewStatus::Failed(_)
            )
        });
        let view = app.world().get::<KuiView>(mount).unwrap();
        let KuiViewStatus::Failed(message) = view.status() else {
            panic!()
        };
        assert!(!message.is_empty());
        assert!(view.roots().is_empty());
        if body.contains("image") {
            let asset = app
                .world()
                .resource::<Assets<KuiAsset>>()
                .get(&handle)
                .unwrap();
            assert_eq!(asset.image_paths, ["missing.png"]);
            assert_eq!(asset.image_handles.len(), 1);
        }
        if body.contains("style") {
            assert!(
                app.world()
                    .resource::<Assets<KuiAsset>>()
                    .get(&handle)
                    .unwrap()
                    .style_sheet
                    .is_some()
            );
        }
    }
}

fn set_visible(app: &mut App, mount: Entity, value: KuiValue) {
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("visible".into(), value);
}

#[test]
fn visibility_restores_headless_display_and_preserves_last_valid_state() {
    let mut app = test_support::app(&[]);
    let handle = asset(
        &mut app,
        r#"<node id="panel" bind:visible="visible"><text bind:text="label"/></node>"#,
    );
    let owner = app.world_mut().spawn_empty().id();
    let mut view = KuiView::new(owner, handle);
    view.data.insert("visible".into(), KuiValue::Boolean(true));
    view.data
        .insert("label".into(), KuiValue::String("before".into()));
    let mount = app.world_mut().spawn(view).id();
    app.update();
    let panel = named(&mut app, "panel");
    let child = app.world().get::<Children>(panel).unwrap()[0];
    app.world_mut().get_mut::<Node>(panel).unwrap().display = Display::Grid;
    app.world_mut().entity_mut(panel).insert(Visibility::Hidden);

    set_visible(&mut app, mount, KuiValue::Boolean(false));
    app.update();
    assert_eq!(
        app.world().get::<Node>(panel).unwrap().display,
        Display::None
    );
    app.world_mut().clear_trackers();
    app.update();
    assert!(
        !app.world()
            .entity(panel)
            .get_ref::<Node>()
            .unwrap()
            .is_changed()
    );

    set_visible(&mut app, mount, KuiValue::String("invalid".into()));
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("label".into(), KuiValue::String("while hidden".into()));
    app.update();
    assert_eq!(
        app.world().get::<Node>(panel).unwrap().display,
        Display::None
    );
    assert_eq!(app.world().get::<Text>(child).unwrap().0, "while hidden");
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .remove("visible");
    app.update();
    assert_eq!(
        app.world().get::<Node>(panel).unwrap().display,
        Display::None
    );

    for _ in 0..3 {
        set_visible(&mut app, mount, KuiValue::Boolean(true));
        app.update();
        assert_eq!(
            app.world().get::<Node>(panel).unwrap().display,
            Display::Grid
        );
        assert_eq!(
            app.world().get::<Visibility>(panel),
            Some(&Visibility::Hidden)
        );
        assert_eq!(app.world().get::<Children>(panel).unwrap()[0], child);
        set_visible(&mut app, mount, KuiValue::Boolean(false));
        app.update();
        assert_eq!(
            app.world().get::<Node>(panel).unwrap().display,
            Display::None
        );
    }
}

pub(crate) fn layout_app(assets: &[(&str, &str)]) -> App {
    let mut app = test_support::app(assets);
    // Render-facing plugins still register their shader assets, but no renderer
    // or window is started for these layout tests.
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
    app.world_mut().spawn((
        Camera2d,
        // RenderPlugin normally computes this target metadata. The headless
        // harness still needs it for logical/physical interaction coordinates.
        Camera {
            computed: bevy::camera::ComputedCameraValues {
                target_info: Some(bevy::camera::RenderTargetInfo {
                    physical_size: UVec2::new(800, 600),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            ..default()
        },
        bevy::camera::RenderTarget::None {
            size: UVec2::new(800, 600),
        },
    ));
    app
}

#[test]
fn widget_buttons_style_hover_and_press_and_dispatch_once_through_pointer_input() {
    use bevy::picking::pointer::{PointerAction, PointerInput};
    use bevy::ui::Pressed;
    use bevy::ui_widgets::Button;

    let source = format!(
        r#"{HEADER}<style src="styles/button.css"/><button id="button" on-click="submit" bind:disabled="disabled"><text>Submit</text></button></view>"#
    );
    let mut app = layout_app(&[
        ("button.kui", &source),
        ("styles/button.css", "@import \"./states.css\";"),
        (
            "styles/states.css",
            "button { width: 100px; height: 40px; } button:hover { width: 120px; } button:active { width: 140px; } button:disabled { width: 80px; }",
        ),
    ]);
    app.add_message::<bevy::window::WindowEvent>()
        .add_plugins(bevy_flair::FlairPlugin);
    #[derive(Resource, Default)]
    struct Actions(Vec<String>);
    app.init_resource::<Actions>().add_observer(
        |event: On<KuiAction>, mut actions: ResMut<Actions>| actions.0.push(event.action.clone()),
    );
    app.finish();
    app.cleanup();
    let owner = app.world_mut().spawn_empty().id();
    let handle = app.world().resource::<AssetServer>().load("button.kui");
    let mut view = KuiView::new(owner, handle);
    view.data
        .insert("disabled".into(), KuiValue::Boolean(false));
    let mount = app.world_mut().spawn(view).id();
    wait_for(&mut app, |app| {
        app.world_mut()
            .query::<(&Name, &ComputedNode)>()
            .iter(app.world())
            .any(|(name, node)| name.as_str() == "button" && node.size().x == 100.0)
    });
    let button = named(&mut app, "button");
    // A windowless render target needs an explicit UI camera.
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<Camera>>()
        .single(app.world())
        .unwrap();
    let roots = app.world().get::<KuiView>(mount).unwrap().roots().to_vec();
    for root in roots {
        app.world_mut()
            .entity_mut(root)
            .insert(UiTargetCamera(camera));
    }
    app.update();
    assert!(app.world().get::<Button>(button).is_some());
    assert!(
        app.world()
            .get::<bevy::picking::hover::Hovered>(button)
            .is_some()
    );
    let input = |app: &mut App, action| {
        app.world_mut().write_message(PointerInput::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::None {
                    width: 800,
                    height: 600,
                },
                position: Vec2::splat(10.0),
            },
            action,
        ));
        app.update();
    };
    input(&mut app, PointerAction::Move { delta: Vec2::ZERO });
    assert_eq!(app.world().get::<Node>(button).unwrap().width, px(120));
    input(&mut app, PointerAction::Press(PointerButton::Primary));
    assert!(app.world().get::<Pressed>(button).is_some());
    assert_eq!(app.world().get::<Node>(button).unwrap().width, px(140));
    input(&mut app, PointerAction::Release(PointerButton::Primary));
    assert!(app.world().get::<Pressed>(button).is_none());
    assert_eq!(app.world().resource::<Actions>().0, ["submit"]);
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("disabled".into(), KuiValue::Boolean(true));
    app.update();
    assert_eq!(app.world().get::<Node>(button).unwrap().width, px(80));
    input(&mut app, PointerAction::Press(PointerButton::Primary));
    input(&mut app, PointerAction::Release(PointerButton::Primary));
    assert!(app.world().get::<Pressed>(button).is_none());
    assert_eq!(app.world().resource::<Actions>().0, ["submit"]);
}

#[test]
fn meters_compute_real_geometry_clamp_animate_and_recover_with_flair() {
    let source = format!(
        r#"{HEADER}<style src="meter.css"/><meter id="range" bind:current="current" bind:maximum="maximum"/></view>"#
    );
    let mut app = layout_app(&[
        ("meter.kui", &source),
        (
            "meter.css",
            ".meter { width: 200px; height: 20px; } .meter-fill { height: 100%; flex-shrink: 0; }",
        ),
    ]);
    app.add_message::<bevy::window::WindowEvent>()
        .add_plugins(bevy_flair::FlairPlugin)
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(100),
        ));
    app.finish();
    app.cleanup();
    let owner = app.world_mut().spawn_empty().id();
    let handle = app.world().resource::<AssetServer>().load("meter.kui");
    let mut view = KuiView::new(owner, handle);
    view.data = BTreeMap::from([
        ("current".into(), KuiValue::Integer(156)),
        ("maximum".into(), KuiValue::Integer(200)),
    ]);
    let mount = app.world_mut().spawn(view).id();
    wait_for(&mut app, |app| {
        app.world_mut()
            .query::<(&ClassList, &ComputedNode)>()
            .iter(app.world())
            .any(|(classes, node)| {
                classes.contains("meter-fill") && (node.size().x - 156.0).abs() < 0.1
            })
    });
    let range = named(&mut app, "range");
    let fill = app.world().get::<Children>(range).unwrap()[0];
    assert_eq!(
        app.world().get::<ComputedNode>(fill).unwrap().size(),
        Vec2::new(156.0, 20.0)
    );
    for (current, maximum, expected) in [
        (300, 200, 200.0),
        (-5, 200, 0.0),
        (9, 0, 0.0),
        (9, -1, 0.0),
        (50, 200, 50.0),
    ] {
        let mut view = app.world_mut().get_mut::<KuiView>(mount).unwrap();
        view.data
            .insert("current".into(), KuiValue::Integer(current));
        view.data
            .insert("maximum".into(), KuiValue::Integer(maximum));
        for _ in 0..15 {
            app.update();
        }
        assert!((app.world().get::<ComputedNode>(fill).unwrap().size().x - expected).abs() < 0.1);
    }
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("current".into(), KuiValue::Number(f64::NAN));
    app.update();
    assert_eq!(
        app.world().get::<Node>(fill).unwrap().width,
        Val::Percent(25.0)
    );
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("current".into(), KuiValue::Integer(100));
    app.update();
    let Val::Percent(animated) = app.world().get::<Node>(fill).unwrap().width else {
        panic!()
    };
    assert!(animated > 25.0 && animated < 50.0);
    for _ in 0..15 {
        app.update();
    }
    assert_eq!(
        app.world().get::<Node>(fill).unwrap().width,
        Val::Percent(50.0)
    );
    app.world_mut().clear_trackers();
    app.update();
    assert!(
        !app.world()
            .entity(fill)
            .get_ref::<Node>()
            .unwrap()
            .is_changed()
    );
}

#[test]
fn component_styles_inherit_theme_variables_and_slots_keep_caller_styles() {
    let source = format!(
        r#"{HEADER}<style src="view.css"/><import name="card" src="card.kui"/><node class="surface"><use component="card" id="instance" class="compact"><fill slot="body"><text id="caller">Caller</text></fill></use></node></view>"#
    );
    let component = r#"<component xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="card"><style src="card.css"/><node class="card"><text id="internal">Component</text><slot name="body"/></node></component>"#;
    let mut app = layout_app(&[
        ("main.kui", &source),
        ("card.kui", component),
        (
            "view.css",
            ".surface { flex-direction: column; width: 300px; --ink: #ff0000; } text { font-size: 28px; } .compact { width: 999px; }",
        ),
        (
            "card.css",
            ".card { width: 180px; flex-direction: column; } .card.compact { width: 120px; } text { font-size: 12px; color: var(--ink); }",
        ),
    ]);
    app.add_message::<bevy::window::WindowEvent>()
        .add_plugins(bevy_flair::FlairPlugin);
    app.finish();
    app.cleanup();
    let owner = app.world_mut().spawn_empty().id();
    let handle = app.world().resource::<AssetServer>().load("main.kui");
    let mount = app.world_mut().spawn(KuiView::new(owner, handle)).id();
    wait_for(&mut app, |app| {
        app.world_mut()
            .query::<(&Name, &TextFont)>()
            .iter(app.world())
            .any(|(name, font)| name.as_str() == "caller" && font.font_size == FontSize::Px(28.0))
    });
    let instance = named(&mut app, "instance");
    let internal = named(&mut app, "internal");
    let caller = named(&mut app, "caller");
    assert_eq!(
        app.world().get::<Node>(instance).unwrap().width,
        Val::Px(120.0)
    );
    assert_eq!(
        app.world().get::<TextFont>(internal).unwrap().font_size,
        FontSize::Px(12.0)
    );
    assert_eq!(
        app.world().get::<TextFont>(caller).unwrap().font_size,
        FontSize::Px(28.0)
    );
    assert_eq!(
        app.world().get::<TextColor>(internal).unwrap().0.to_srgba(),
        Srgba::RED
    );
    assert_eq!(
        app.world().get::<ChildOf>(caller).unwrap().parent(),
        instance
    );
    let mut ancestor = instance;
    while ancestor != mount {
        ancestor = app
            .world()
            .get::<ChildOf>(ancestor)
            .expect("native mount ancestry")
            .parent();
    }
}

#[test]
fn stylesheet_on_nested_component_root_survives_unstyled_wrappers() {
    let source = format!(
        r#"{HEADER}<style src="view.css"/><import name="outer" src="outer.kui"/><use component="outer"/></view>"#
    );
    let outer = r#"<component xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="outer"><import name="inner" src="inner.kui"/><use component="inner"/></component>"#;
    let inner = r#"<component xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="inner"><style src="inner.css"/><node id="inner"/></component>"#;
    let mut app = layout_app(&[
        ("main.kui", &source),
        ("outer.kui", outer),
        ("inner.kui", inner),
        ("view.css", "node { width: 999px; }"),
        ("inner.css", "node { width: 120px; height: 20px; }"),
    ]);
    app.add_message::<bevy::window::WindowEvent>()
        .add_plugins(bevy_flair::FlairPlugin);
    app.finish();
    app.cleanup();
    let owner = app.world_mut().spawn_empty().id();
    let handle = app.world().resource::<AssetServer>().load("main.kui");
    app.world_mut().spawn(KuiView::new(owner, handle));
    wait_for(&mut app, |app| {
        app.world_mut()
            .query::<(&Name, &Node)>()
            .iter(app.world())
            .any(|(name, node)| name.as_str() == "inner" && node.width == Val::Px(120.0))
    });
    let inner = named(&mut app, "inner");
    assert_eq!(
        app.world().get::<ComputedNode>(inner).unwrap().size(),
        Vec2::new(120.0, 20.0)
    );
}

#[test]
fn hidden_subtree_collapses_layout_and_showing_restores_sibling_positions() {
    let mut app = layout_app(&[]);
    let handle = asset(
        &mut app,
        r#"<node id="column"><node id="first"/><node id="middle" bind:visible="visible"><node id="nested"/></node><node id="last"/></node>"#,
    );
    let owner = app.world_mut().spawn_empty().id();
    let mut view = KuiView::new(owner, handle);
    view.data.insert("visible".into(), KuiValue::Boolean(true));
    let mount = app.world_mut().spawn(view).id();
    app.finish();
    app.cleanup();
    app.update();
    let column = named(&mut app, "column");
    let middle = named(&mut app, "middle");
    let nested = named(&mut app, "nested");
    let last = named(&mut app, "last");
    app.world_mut().entity_mut(column).insert(Node {
        width: Val::Px(100.0),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(5.0),
        ..default()
    });
    for name in ["first", "middle", "last", "nested"] {
        let entity = named(&mut app, name);
        app.world_mut().get_mut::<Node>(entity).unwrap().height = Val::Px(20.0);
    }
    app.update();
    let geometry = |app: &App| {
        (
            app.world().get::<ComputedNode>(column).unwrap().size().y,
            app.world()
                .get::<UiGlobalTransform>(last)
                .unwrap()
                .translation
                .y,
        )
    };
    assert_eq!(geometry(&app), (70.0, 60.0));
    set_visible(&mut app, mount, KuiValue::Boolean(false));
    app.update();
    assert_eq!(geometry(&app), (45.0, 35.0));
    assert_eq!(
        app.world().get::<ComputedNode>(middle).unwrap().size(),
        Vec2::ZERO
    );
    assert_eq!(
        app.world().get::<ComputedNode>(nested).unwrap().size(),
        Vec2::ZERO
    );
    app.update();
    assert_eq!(geometry(&app), (45.0, 35.0));
    set_visible(&mut app, mount, KuiValue::Boolean(true));
    app.update();
    assert_eq!(geometry(&app), (70.0, 60.0));
    assert_eq!(app.world().get::<Children>(middle).unwrap()[0], nested);
}

#[test]
fn visibility_respects_flair_display_changes_and_inline_styles() {
    use bevy_flair::{FlairPlugin, prelude::InlineStyle};
    let source = format!(
        r#"{HEADER}<style src="panel.css"/><node id="panel" class="panel" bind:visible="visible"/></view>"#
    );
    let mut app = layout_app(&[
        ("panel.kui", &source),
        ("panel.css", ".panel { display: grid; }"),
    ]);
    app.add_message::<bevy::window::WindowEvent>()
        .add_plugins(FlairPlugin);
    app.finish();
    app.cleanup();
    let handle = app
        .world()
        .resource::<AssetServer>()
        .load::<KuiAsset>("panel.kui");
    let owner = app.world_mut().spawn_empty().id();
    let mut view = KuiView::new(owner, handle);
    view.data.insert("visible".into(), KuiValue::Boolean(true));
    let mount = app.world_mut().spawn(view).id();
    wait_for(&mut app, |app| {
        app.world_mut()
            .query::<(&Name, &Node)>()
            .iter(app.world())
            .any(|(name, node)| name.as_str() == "panel" && node.display == Display::Grid)
    });
    let panel = named(&mut app, "panel");
    app.world_mut().entity_mut(panel).insert(InlineStyle::new(
        "display: flex; width: 73px; visibility: hidden;",
    ));
    wait_for(&mut app, |app| {
        app.world().get::<Node>(panel).unwrap().display == Display::Flex
            && app.world().get::<Node>(panel).unwrap().width == Val::Px(73.0)
    });
    set_visible(&mut app, mount, KuiValue::Boolean(false));
    app.update();
    assert_eq!(
        app.world().get::<Node>(panel).unwrap().display,
        Display::None
    );
    app.world_mut()
        .get_mut::<InlineStyle>(panel)
        .unwrap()
        .set("display", "grid");
    app.world_mut()
        .get_mut::<InlineStyle>(panel)
        .unwrap()
        .set("width", "91px");
    wait_for(&mut app, |app| {
        assert_eq!(
            app.world().get::<Node>(panel).unwrap().display,
            Display::None
        );
        app.world().get::<Node>(panel).unwrap().width == Val::Px(91.0)
    });
    set_visible(&mut app, mount, KuiValue::Boolean(true));
    app.update();
    assert_eq!(
        app.world().get::<Node>(panel).unwrap().display,
        Display::Grid
    );
    assert_eq!(app.world().get::<Node>(panel).unwrap().width, Val::Px(91.0));
    assert_eq!(
        app.world().get::<Visibility>(panel),
        Some(&Visibility::Hidden)
    );
    app.world_mut()
        .get_mut::<InlineStyle>(panel)
        .unwrap()
        .set("display", "none");
    wait_for(&mut app, |app| {
        app.world().get::<Node>(panel).unwrap().display == Display::None
    });
    set_visible(&mut app, mount, KuiValue::Boolean(false));
    app.update();
    set_visible(&mut app, mount, KuiValue::Boolean(true));
    app.update();
    assert_eq!(
        app.world().get::<Node>(panel).unwrap().display,
        Display::None
    );
}

#[test]
fn stylesheet_reload_stays_hidden_and_showing_uses_reloaded_display() {
    use bevy_flair::FlairPlugin;
    let source = format!(
        r#"{HEADER}<style src="panel.css"/><node id="panel" class="panel" bind:visible="visible"/></view>"#
    );
    let mut app = layout_app(&[
        ("panel.kui", &source),
        ("panel.css", ".panel { display: grid; width: 30px; }"),
        ("replacement.css", ".panel { display: block; width: 60px; }"),
    ]);
    app.add_plugins(FlairPlugin);
    app.finish();
    app.cleanup();
    let server = app.world().resource::<AssetServer>();
    let handle = server.load::<KuiAsset>("panel.kui");
    let replacement = server.load::<StyleSheet>("replacement.css");
    let owner = app.world_mut().spawn_empty().id();
    let mut view = KuiView::new(owner, handle.clone());
    view.data.insert("visible".into(), KuiValue::Boolean(true));
    let mount = app.world_mut().spawn(view).id();
    wait_for(&mut app, |app| {
        app.world_mut()
            .query::<(&Name, &Node)>()
            .iter(app.world())
            .any(|(name, node)| name.as_str() == "panel" && node.display == Display::Grid)
            && app
                .world()
                .resource::<AssetServer>()
                .is_loaded_with_dependencies(&replacement)
    });
    let panel = named(&mut app, "panel");
    set_visible(&mut app, mount, KuiValue::Boolean(false));
    app.update();
    let style = app
        .world()
        .resource::<Assets<KuiAsset>>()
        .get(&handle)
        .unwrap()
        .style_sheet
        .clone()
        .unwrap();
    {
        let mut styles = app.world_mut().resource_mut::<Assets<StyleSheet>>();
        let replacement = styles.get(&replacement).unwrap().clone();
        *styles.get_mut(&style).unwrap() = replacement;
    }
    wait_for(&mut app, |app| {
        let node = app.world().get::<Node>(panel).unwrap();
        assert_eq!(node.display, Display::None);
        node.width == Val::Px(60.0)
    });
    set_visible(&mut app, mount, KuiValue::Boolean(true));
    app.update();
    assert_eq!(
        app.world().get::<Node>(panel).unwrap().display,
        Display::Block
    );
}
