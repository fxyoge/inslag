use bevy::prelude::*;
use bevy_flair::{
    core::{
        CssPropertyRegistry, PropertyRegistry, ReflectStructPropertyRefExt,
        RegisterComponentPropertiesExt,
    },
    parser::{ReflectParseCss, parse_property_value_with},
    prelude::{AnimationProgress, ComponentProperties, StyleProperties},
    style::StyleSystems,
};

use crate::{KuiComponentInstance, KuiSystems, KuiViewRoot};

#[derive(Reflect, Clone, Debug, Default, PartialEq)]
#[reflect(Clone, Debug, Default, PartialEq)]
struct TimelineName(String);

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RestorePresentation;

#[derive(Component, ComponentProperties, Reflect, Clone, Debug, Default, PartialEq)]
#[properties(auto_insert_remove)]
#[reflect(Component, Clone, Debug, Default, PartialEq)]
struct MotionStyle {
    timeline: TimelineName,
    source: TimelineName,
    progress: f32,
    origin: Vec2,
}

#[derive(Resource)]
struct MotionProperties {
    timeline: bevy_flair::core::ComponentPropertyId,
    source: bevy_flair::core::ComponentPropertyId,
    progress: bevy_flair::core::ComponentPropertyId,
}

#[derive(Component, Default)]
pub(crate) struct WorldPresentation {
    base: Transform,
    written: Option<Transform>,
}

#[derive(Component)]
struct TimelineControl;

#[derive(Component)]
struct LayoutOrigin {
    base: UiTransform,
    written: UiTransform,
}

pub(crate) fn install(app: &mut App) {
    // Asset-only/headless KUI remains usable without Flair.
    if !app.world().contains_resource::<CssPropertyRegistry>()
        || app.world().contains_resource::<MotionProperties>()
    {
        return;
    }
    app.register_type::<TimelineName>()
        .register_type::<MotionStyle>()
        .register_component_properties::<MotionStyle>();
    app.world()
        .resource::<AppTypeRegistry>()
        .write()
        .get_mut(std::any::TypeId::of::<TimelineName>())
        .unwrap()
        .insert(ReflectParseCss(|parser| {
            parse_property_value_with(parser, |parser| {
                Ok(bevy_flair::core::ReflectValue::new(TimelineName(
                    parser.expect_ident_or_string()?.to_string(),
                )))
            })
        }));
    let css = app.world().resource::<CssPropertyRegistry>();
    css.register_property("-kui-timeline", MotionStyle::property_field_ref("timeline"));
    css.register_property(
        "-kui-animation-timeline",
        MotionStyle::property_field_ref("source"),
    );
    css.register_property("-kui-progress", MotionStyle::property_field_ref("progress"));
    css.register_property("-kui-origin", MotionStyle::property_field_ref("origin"));
    let registry = app.world().resource::<PropertyRegistry>();
    let ids = MotionProperties {
        timeline: registry
            .resolve(MotionStyle::property_field_ref("timeline"))
            .unwrap(),
        source: registry
            .resolve(MotionStyle::property_field_ref("source"))
            .unwrap(),
        progress: registry
            .resolve(MotionStyle::property_field_ref("progress"))
            .unwrap(),
    };
    app.insert_resource(ids)
        .add_systems(
            Update,
            restore_world
                .in_set(RestorePresentation)
                .before(KuiSystems::Prepare),
        )
        .add_systems(PostUpdate, restore_layout.before(StyleSystems::Prepare))
        .add_systems(
            PostUpdate,
            connect_timelines
                .after(StyleSystems::ResolveAnimations)
                .before(StyleSystems::SetAnimationValues),
        )
        .add_systems(
            PostUpdate,
            (apply_world, apply_layout_origin)
                .after(StyleSystems::ApplyComputedProperties)
                .before(bevy::ui::UiSystems::Content)
                .before(bevy::transform::TransformSystems::Propagate),
        );
}

fn connect_timelines(
    mut commands: Commands,
    ids: Res<MotionProperties>,
    styles: Query<(
        Entity,
        &StyleProperties,
        Option<&AnimationProgress>,
        Has<TimelineControl>,
    )>,
    parents: Query<&ChildOf>,
    boundaries: Query<(), Or<(With<KuiViewRoot>, With<KuiComponentInstance>)>>,
) {
    let string = |style: &StyleProperties, id| {
        style
            .sample_property(id)
            .and_then(|v| v.downcast_value_ref::<TimelineName>().map(|n| n.0.clone()))
    };
    for (entity, style, previous, owned) in &styles {
        let source = string(style, ids.source).filter(|s| !s.is_empty() && s != "none");
        let progress = source.and_then(|source| {
            let mut ancestor = Some(entity);
            while let Some(current) = ancestor {
                if let Ok((_, style, _, _)) = styles.get(current)
                    && string(style, ids.timeline).as_deref() == Some(source.as_str())
                {
                    return style
                        .sample_property(ids.progress)
                        .and_then(|v| v.downcast_value_ref::<f32>().copied())
                        .and_then(AnimationProgress::new)
                        .or_else(|| AnimationProgress::new(0.0));
                }
                if boundaries.contains(current) {
                    break;
                }
                ancestor = parents.get(current).ok().map(ChildOf::parent);
            }
            warn_once!("KUI animation timeline `{source}` has no source in its presentation scope");
            // Keep the effect at its initial pose rather than accidentally running
            // its clock when a source is missing.
            AnimationProgress::new(0.0)
        });
        match progress {
            Some(progress) if previous != Some(&progress) || !owned => {
                commands.entity(entity).insert((progress, TimelineControl));
            }
            None if owned => {
                commands
                    .entity(entity)
                    .remove::<(AnimationProgress, TimelineControl)>();
            }
            _ => {}
        }
    }
}

fn restore_world(mut worlds: Query<(&mut Transform, &WorldPresentation)>) {
    for (mut transform, state) in &mut worlds {
        if state.written.as_ref() == Some(&*transform) {
            // Only the final presentation should trigger propagation. The
            // temporary baseline is restored before bindings/interaction run.
            *transform.bypass_change_detection() = state.base;
        }
    }
}

fn apply_world(
    mut worlds: Query<(
        &mut Transform,
        &UiTransform,
        &mut WorldPresentation,
        Option<&MotionStyle>,
    )>,
) {
    for (mut transform, css, mut state, motion) in &mut worlds {
        state.base = *transform;
        let mut displayed = state.base;
        let pixels = |value| match value {
            Val::Px(value) => Some(value),
            Val::Auto => Some(0.0),
            _ => None,
        };
        let Some((x, y)) = pixels(css.translation.x).zip(pixels(css.translation.y)) else {
            warn_once!(
                "World CSS translate requires pixel lengths; relative/viewport units are unsupported"
            );
            if state.written != Some(displayed) {
                transform.set_changed();
            }
            state.written = Some(displayed);
            continue;
        };
        let origin = motion.map_or(Vec2::ZERO, |m| m.origin);
        let offset = Vec2::new(x, y) + origin - css.rotation * (css.scale * origin);
        displayed.translation += Vec3::new(offset.x, -offset.y, 0.0);
        displayed.scale *= css.scale.extend(1.0);
        displayed.rotation *= Quat::from_rotation_z(-css.rotation.as_radians());
        if state.written == Some(displayed) {
            *transform.bypass_change_detection() = displayed;
        } else {
            *transform = displayed;
        }
        state.written = Some(displayed);
    }
}

fn restore_layout(mut origins: Query<(&mut UiTransform, &LayoutOrigin)>) {
    for (mut transform, origin) in &mut origins {
        if *transform == origin.written {
            *transform = origin.base;
        }
    }
}

fn apply_layout_origin(
    mut commands: Commands,
    mut layouts: Query<
        (
            Entity,
            &mut UiTransform,
            Option<&MotionStyle>,
            Has<LayoutOrigin>,
        ),
        (With<Node>, Or<(With<MotionStyle>, With<LayoutOrigin>)>),
    >,
) {
    for (entity, mut transform, motion, owned) in &mut layouts {
        let origin = motion.map_or(Vec2::ZERO, |m| m.origin);
        if origin == Vec2::ZERO {
            if owned {
                commands.entity(entity).remove::<LayoutOrigin>();
            }
            continue;
        }
        let base = *transform;
        let offset = origin - transform.rotation * (transform.scale * origin);
        let Ok(translation) = transform.translation.try_add(Val2::px(offset.x, offset.y)) else {
            warn_once!("KUI pixel origin requires pixel translation on layout elements");
            continue;
        };
        transform.translation = translation;
        commands.entity(entity).insert(LayoutOrigin {
            base,
            written: *transform,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_origin_filter_still_restores_nodes_after_style_removal() {
        let mut app = App::new();
        app.add_systems(Update, (restore_layout, apply_layout_origin).chain());
        let node = app
            .world_mut()
            .spawn((Node::default(), UiTransform::from_scale(Vec2::splat(2.0))))
            .id();
        app.update();
        assert!(app.world().get::<LayoutOrigin>(node).is_none());
        app.world_mut().entity_mut(node).insert(MotionStyle {
            origin: Vec2::new(10.0, 0.0),
            ..default()
        });
        app.update();
        assert_eq!(
            app.world().get::<UiTransform>(node).unwrap().translation.x,
            Val::Px(-10.0)
        );
        app.world_mut().entity_mut(node).remove::<MotionStyle>();
        app.update();
        assert_eq!(
            app.world().get::<UiTransform>(node).unwrap().translation.x,
            Val::Px(0.0)
        );
        assert!(app.world().get::<LayoutOrigin>(node).is_none());
    }

    #[test]
    fn motion_properties_register_before_flair_freezes_the_registry() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            // Production order differs from the asset-only test helpers: Flair
            // finishes first, so registration cannot be deferred to KUI's finish.
            .add_plugins((bevy_flair::FlairPlugin, crate::KuiPlugin));
        app.finish();
        assert!(
            app.world()
                .resource::<CssPropertyRegistry>()
                .get_property("-kui-progress")
                .is_some()
        );
    }
}
