//! The same Aseprite CSS source targets sprites in world presentations.
use bevy::{prelude::*, sprite::Anchor};
use bevy_flair::{
    parser::{CalcValue, CssError, Parser, parse_calc},
    prelude::ComponentProperties,
};

use super::{AsepriteImage, AsepriteImageStatus, entity_label};
use bevy_aseprite_ultra::prelude::Aseprite;

#[derive(Component, ComponentProperties, Reflect, Clone, Debug, PartialEq)]
#[properties(auto_insert_remove)]
#[reflect(Component, Clone, Debug, Default, PartialEq)]
pub(super) struct SpriteStyle {
    scale: f32,
    pivot_offset: Vec2,
    crop: Option<Rect>,
    size: Option<Vec2>,
}

impl Default for SpriteStyle {
    fn default() -> Self {
        Self {
            scale: 1.0,
            pivot_offset: Vec2::ZERO,
            crop: None,
            size: None,
        }
    }
}

pub(super) fn parse_size(parser: &mut Parser) -> Result<Option<Vec2>, CssError> {
    if parser
        .try_parse(|parser| parser.expect_ident_matching("none"))
        .is_ok()
    {
        return Ok(None);
    }
    fn length(parser: &mut Parser) -> Result<f32, CssError> {
        parse_calc(parser, |value| match value {
            CalcValue::Number(value) => Ok(*value),
            CalcValue::Dimension { value, dim } if dim.eq_ignore_ascii_case("px") => Ok(*value),
            _ => Err("expected a number or pixel length".to_owned()),
        })
    }
    let x = length(parser)?;
    let y = parser.try_parse(length).unwrap_or(x);
    Ok(Some(Vec2::new(x, y)))
}

// Only restore fields this adapter owns. Color, flip, and transforms remain
// available to independent presentation systems throughout the source's life.
#[derive(Clone, PartialEq)]
struct SpriteFields {
    image: Handle<Image>,
    texture_atlas: Option<TextureAtlas>,
    rect: Option<Rect>,
    size: Option<Vec2>,
    mode: SpriteImageMode,
    anchor: Anchor,
}

impl SpriteFields {
    fn capture(sprite: &Sprite, anchor: Anchor) -> Self {
        Self {
            image: sprite.image.clone(),
            texture_atlas: sprite.texture_atlas.clone(),
            rect: sprite.rect,
            size: sprite.custom_size,
            mode: sprite.image_mode.clone(),
            anchor,
        }
    }

    fn restore(&self, sprite: &mut Sprite, anchor: &mut Anchor) {
        sprite.image = self.image.clone();
        sprite.texture_atlas = self.texture_atlas.clone();
        sprite.rect = self.rect;
        sprite.custom_size = self.size;
        sprite.image_mode = self.mode.clone();
        *anchor = self.anchor;
    }
}

#[derive(Component)]
pub(super) struct AppliedSprite {
    baseline: Option<SpriteFields>,
    original_anchor: Option<Anchor>,
    last_written: Option<SpriteFields>,
    last_style: Option<SpriteStyle>,
    status: AsepriteImageStatus,
}

pub(super) fn prepare(
    mut commands: Commands,
    sources: Query<
        (Entity, Option<&Sprite>, Option<&Anchor>),
        (
            With<AsepriteImage>,
            With<crate::motion::WorldPresentation>,
            Without<AppliedSprite>,
        ),
    >,
) {
    for (entity, sprite, anchor) in &sources {
        let mut entity = commands.entity(entity);
        entity.insert(AppliedSprite {
            baseline: sprite
                .map(|sprite| SpriteFields::capture(sprite, anchor.copied().unwrap_or_default())),
            original_anchor: anchor.copied(),
            last_written: None,
            last_style: None,
            status: AsepriteImageStatus::Pending,
        });
        if sprite.is_none() {
            entity.insert(Sprite {
                custom_size: Some(Vec2::ZERO),
                ..default()
            });
        }
    }
}

pub(super) fn apply(
    assets: Res<Assets<Aseprite>>,
    server: Res<AssetServer>,
    mut sprites: Query<(
        Entity,
        Ref<AsepriteImage>,
        Option<&SpriteStyle>,
        &mut Sprite,
        &mut Anchor,
        &mut AppliedSprite,
        Option<&Name>,
    )>,
) {
    for (entity, source, style, mut sprite, mut anchor, mut applied, name) in &mut sprites {
        let fallback = SpriteStyle::default();
        let style = style.unwrap_or(&fallback);
        if !assets.is_changed()
            && !source.is_changed()
            && applied.last_style.as_ref() == Some(style)
            && !matches!(
                applied.status,
                AsepriteImageStatus::Pending | AsepriteImageStatus::Loading
            )
        {
            continue;
        }
        applied.last_style = Some(style.clone());
        let (status, message) = if let Some(asset) = assets.get(&source.aseprite) {
            if let Some(slice) = asset.slices.get(&source.slice) {
                let full = Rect::from_corners(Vec2::ZERO, slice.rect.size());
                let visible = style.crop.map_or(full, |crop| full.intersect(crop));
                let size = style.size.unwrap_or(visible.size() * style.scale);
                if style.scale.is_finite()
                    && style.scale > 0.0
                    && style.pivot_offset.is_finite()
                    && visible.size().is_finite()
                    && visible.size().min_element() > 0.0
                    && size.is_finite()
                    && size.min_element() > 0.0
                {
                    let mode = if let Some(center) = slice.nine_patch {
                        // Cropping away borders changes the nine-patch contract.
                        // Reject it rather than slicing a different rectangle.
                        let Some(mut slicer) =
                            super::texture_slicer(slice, center).filter(|_| visible == full)
                        else {
                            sprite.custom_size = Some(Vec2::ZERO);
                            applied.last_written = Some(SpriteFields::capture(&sprite, *anchor));
                            if applied.status != AsepriteImageStatus::InvalidNinePatch {
                                error!(
                                    "Aseprite world image for {} has invalid or cropped nine-patch metadata",
                                    entity_label(entity, name)
                                );
                            }
                            applied.status = AsepriteImageStatus::InvalidNinePatch;
                            continue;
                        };
                        slicer.max_corner_scale = style.scale;
                        SpriteImageMode::Sliced(slicer)
                    } else {
                        SpriteImageMode::Auto
                    };
                    let pivot = slice.pivot.unwrap_or(full.center()) + style.pivot_offset;
                    sprite.image = asset.atlas_image.clone();
                    sprite.texture_atlas = None;
                    sprite.rect = Some(Rect::from_corners(
                        slice.rect.min + visible.min,
                        slice.rect.min + visible.max,
                    ));
                    sprite.custom_size = Some(size);
                    sprite.image_mode = mode;
                    // Registration pivots may be outside the image. Do not clamp
                    // them: detached pieces must retain their shared origin.
                    anchor.0 = ((pivot - visible.min) / visible.size() - Vec2::splat(0.5))
                        * Vec2::new(1.0, -1.0);
                    applied.last_written = Some(SpriteFields::capture(&sprite, *anchor));
                    applied.status = AsepriteImageStatus::Ready;
                    continue;
                }
                (
                    AsepriteImageStatus::InvalidLayout,
                    Some("invalid scale, pivot offset, crop, or size".to_owned()),
                )
            } else {
                (
                    AsepriteImageStatus::MissingSlice,
                    Some(format!("missing slice {:?}", source.slice)),
                )
            }
        } else if let bevy::asset::LoadState::Failed(error) =
            server.load_state(source.aseprite.id())
        {
            (AsepriteImageStatus::Failed, Some(error.to_string()))
        } else {
            (AsepriteImageStatus::Loading, None)
        };
        if (applied.status != status || source.is_changed())
            && let Some(message) = message
        {
            error!(
                "Aseprite world image for {}: {message}",
                entity_label(entity, name)
            );
        }
        sprite.custom_size = Some(Vec2::ZERO);
        applied.last_written = Some(SpriteFields::capture(&sprite, *anchor));
        applied.status = status;
    }
}

pub(super) fn cleanup(
    mut commands: Commands,
    mut removed: RemovedComponents<AsepriteImage>,
    mut sprites: Query<(
        &mut Sprite,
        &mut Anchor,
        &AppliedSprite,
        Option<&AsepriteImage>,
    )>,
) {
    for entity in removed.read() {
        let Ok((mut sprite, mut anchor, applied, source)) = sprites.get_mut(entity) else {
            continue;
        };
        if source.is_some() {
            continue;
        }
        if applied
            .last_written
            .as_ref()
            .is_some_and(|last| *last != SpriteFields::capture(&sprite, *anchor))
        {
            commands.entity(entity).remove::<AppliedSprite>();
            continue;
        }
        if let Some(baseline) = &applied.baseline {
            baseline.restore(&mut sprite, &mut anchor);
        } else {
            commands.entity(entity).remove::<Sprite>();
            if let Some(original) = applied.original_anchor {
                *anchor = original;
            } else {
                commands.entity(entity).remove::<Anchor>();
            }
        }
        commands.entity(entity).remove::<AppliedSprite>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::uuid_handle;
    use bevy_aseprite_ultra::prelude::SliceMeta;

    const IMAGE: Handle<Image> = uuid_handle!("32765ff2-1c6a-438f-8aaa-98f7b270568e");

    fn app() -> (App, Handle<Aseprite>) {
        let mut app = crate::runtime::test_support::app(&[]);
        app.init_asset::<Aseprite>().add_systems(
            Update,
            (
                super::super::prepare_aseprite_image_nodes,
                super::super::apply_aseprite_images,
                super::super::cleanup_removed_aseprite_images,
                prepare,
                apply,
                cleanup,
            )
                .chain(),
        );
        let mut asset = Aseprite::default();
        asset.atlas_image = IMAGE;
        asset.slices.insert(
            "part".into(),
            SliceMeta {
                rect: Rect::new(10.0, 20.0, 18.0, 26.0),
                atlas_id: 0,
                pivot: Some(Vec2::new(-4.0, 13.0)),
                nine_patch: None,
                keys: vec![],
            },
        );
        let handle = app
            .world_mut()
            .resource_mut::<Assets<Aseprite>>()
            .add(asset);
        (app, handle)
    }

    #[test]
    fn world_nodes_use_unclamped_pivots_crop_and_scale_and_clean_up() {
        let (mut app, handle) = app();
        let entity = app
            .world_mut()
            .spawn((
                Transform::default(),
                crate::motion::WorldPresentation::default(),
                AsepriteImage {
                    aseprite: handle.clone(),
                    slice: "part".into(),
                },
                SpriteStyle {
                    scale: 3.0,
                    pivot_offset: Vec2::splat(0.5),
                    crop: Some(Rect::new(1.0, 2.0, 7.0, 6.0)),
                    ..default()
                },
            ))
            .id();
        app.update();
        let sprite = app.world().get::<Sprite>(entity).unwrap();
        assert_eq!(sprite.image, IMAGE);
        assert_eq!(sprite.rect, Some(Rect::new(11.0, 22.0, 17.0, 26.0)));
        assert_eq!(sprite.custom_size, Some(Vec2::new(18.0, 12.0)));
        assert_eq!(
            app.world().get::<Anchor>(entity).unwrap().0,
            Vec2::new(-1.25, -2.375)
        );
        assert!(app.world().get::<ImageNode>(entity).is_none());
        assert!(app.world().get::<Node>(entity).is_none());

        app.world_mut().get_mut::<Sprite>(entity).unwrap().color = Color::BLACK;
        app.world_mut()
            .resource_mut::<Assets<Aseprite>>()
            .get_mut(&handle)
            .unwrap()
            .slices
            .get_mut("part")
            .unwrap()
            .rect = Rect::new(30.0, 40.0, 38.0, 46.0);
        app.update();
        let sprite = app.world().get::<Sprite>(entity).unwrap();
        assert_eq!(sprite.rect, Some(Rect::new(31.0, 42.0, 37.0, 46.0)));
        assert_eq!(sprite.color, Color::BLACK);

        app.world_mut().entity_mut(entity).remove::<SpriteStyle>();
        app.update();
        assert_eq!(
            app.world().get::<Sprite>(entity).unwrap().custom_size,
            Some(Vec2::new(8.0, 6.0))
        );
        app.world_mut()
            .get_mut::<AsepriteImage>(entity)
            .unwrap()
            .slice = "missing".into();
        app.update();
        assert_eq!(
            app.world().get::<Sprite>(entity).unwrap().custom_size,
            Some(Vec2::ZERO)
        );
        app.world_mut()
            .get_mut::<AsepriteImage>(entity)
            .unwrap()
            .slice = "part".into();
        app.update();
        assert_eq!(
            app.world().get::<Sprite>(entity).unwrap().custom_size,
            Some(Vec2::new(8.0, 6.0))
        );
        app.world_mut().entity_mut(entity).remove::<AsepriteImage>();
        app.update();
        assert!(app.world().get::<Sprite>(entity).is_none());
        assert!(app.world().get::<Anchor>(entity).is_none());
        assert!(app.world().get::<Transform>(entity).is_some());
    }

    #[test]
    fn nine_patch_resizes_reloads_and_restores_without_stretching_corners() {
        let (mut app, handle) = app();
        {
            let mut assets = app.world_mut().resource_mut::<Assets<Aseprite>>();
            let mut asset = assets.get_mut(&handle).unwrap();
            let slice = asset.slices.get_mut("part").unwrap();
            slice.pivot = None;
            slice.nine_patch = Some(Vec4::new(2.0, 1.0, 3.0, 3.0));
        }
        let baseline = Sprite {
            custom_size: Some(Vec2::new(19.0, 23.0)),
            ..default()
        };
        let fields = SpriteFields::capture(&baseline, Anchor::CENTER);
        let entity = app
            .world_mut()
            .spawn((
                baseline,
                Anchor::CENTER,
                crate::motion::WorldPresentation::default(),
                AsepriteImage {
                    aseprite: handle.clone(),
                    slice: "part".into(),
                },
                SpriteStyle {
                    size: Some(Vec2::new(70.0, 40.0)),
                    ..default()
                },
            ))
            .id();
        app.update();
        let sprite = app.world().get::<Sprite>(entity).unwrap();
        assert_eq!(sprite.custom_size, Some(Vec2::new(70.0, 40.0)));
        assert_eq!(sprite.rect, Some(Rect::new(10.0, 20.0, 18.0, 26.0)));
        let SpriteImageMode::Sliced(slicer) = &sprite.image_mode else {
            panic!("expected sliced world sprite")
        };
        assert_eq!(slicer.border, BorderRect::from([2.0, 3.0, 1.0, 2.0]));
        assert_eq!(slicer.max_corner_scale, 1.0);
        assert_eq!(*app.world().get::<Anchor>(entity).unwrap(), Anchor::CENTER);
        app.world_mut().get_mut::<SpriteStyle>(entity).unwrap().size = Some(Vec2::new(12.0, 80.0));
        app.update();
        assert_eq!(
            app.world().get::<Sprite>(entity).unwrap().custom_size,
            Some(Vec2::new(12.0, 80.0))
        );
        app.world_mut()
            .resource_mut::<Assets<Aseprite>>()
            .get_mut(&handle)
            .unwrap()
            .slices
            .get_mut("part")
            .unwrap()
            .nine_patch = Some(Vec4::new(1.0, 2.0, 5.0, 2.0));
        app.update();
        let SpriteImageMode::Sliced(slicer) =
            &app.world().get::<Sprite>(entity).unwrap().image_mode
        else {
            panic!("expected reloaded slicing")
        };
        assert_eq!(slicer.border, BorderRect::from([1.0, 2.0, 2.0, 2.0]));
        app.world_mut().entity_mut(entity).remove::<AsepriteImage>();
        app.update();
        assert!(
            SpriteFields::capture(
                app.world().get::<Sprite>(entity).unwrap(),
                *app.world().get::<Anchor>(entity).unwrap()
            ) == fields
        );
    }

    #[test]
    fn invalid_nine_patch_and_size_hide_then_recover() {
        let (mut app, handle) = app();
        let entity = app
            .world_mut()
            .spawn((
                crate::motion::WorldPresentation::default(),
                AsepriteImage {
                    aseprite: handle.clone(),
                    slice: "part".into(),
                },
                SpriteStyle::default(),
            ))
            .id();
        for center in [
            Vec4::new(9.0, 1.0, 1.0, 1.0),
            Vec4::new(1.0, 1.0, 0.0, 1.0),
            Vec4::splat(f32::NAN),
        ] {
            app.world_mut()
                .resource_mut::<Assets<Aseprite>>()
                .get_mut(&handle)
                .unwrap()
                .slices
                .get_mut("part")
                .unwrap()
                .nine_patch = Some(center);
            app.update();
            assert_eq!(
                app.world().get::<Sprite>(entity).unwrap().custom_size,
                Some(Vec2::ZERO)
            );
        }
        app.world_mut()
            .resource_mut::<Assets<Aseprite>>()
            .get_mut(&handle)
            .unwrap()
            .slices
            .get_mut("part")
            .unwrap()
            .nine_patch = Some(Vec4::new(1.0, 1.0, 6.0, 4.0));
        app.update();
        assert!(matches!(
            app.world().get::<Sprite>(entity).unwrap().image_mode,
            SpriteImageMode::Sliced(_)
        ));
        for size in [Vec2::ZERO, Vec2::new(-1.0, 5.0), Vec2::splat(f32::INFINITY)] {
            app.world_mut().get_mut::<SpriteStyle>(entity).unwrap().size = Some(size);
            app.update();
            assert_eq!(
                app.world().get::<Sprite>(entity).unwrap().custom_size,
                Some(Vec2::ZERO)
            );
        }
        app.world_mut().get_mut::<SpriteStyle>(entity).unwrap().size = None;
        app.world_mut().get_mut::<SpriteStyle>(entity).unwrap().crop =
            Some(Rect::new(1.0, 0.0, 8.0, 6.0));
        app.update();
        assert_eq!(
            app.world().get::<Sprite>(entity).unwrap().custom_size,
            Some(Vec2::ZERO)
        );
        app.world_mut().get_mut::<SpriteStyle>(entity).unwrap().crop = None;
        app.update();
        assert_eq!(
            app.world().get::<Sprite>(entity).unwrap().custom_size,
            Some(Vec2::new(8.0, 6.0))
        );
    }

    #[test]
    fn removing_source_restores_existing_sprite_fields_without_overwriting_color() {
        let (mut app, handle) = app();
        let baseline = Sprite {
            custom_size: Some(Vec2::new(91.0, 57.0)),
            rect: Some(Rect::new(2.0, 3.0, 9.0, 8.0)),
            ..default()
        };
        let anchor = Anchor(Vec2::new(0.2, -0.3));
        let fields = SpriteFields::capture(&baseline, anchor);
        let entity = app
            .world_mut()
            .spawn((
                baseline,
                anchor,
                crate::motion::WorldPresentation::default(),
                AsepriteImage {
                    aseprite: handle,
                    slice: "part".into(),
                },
            ))
            .id();
        app.update();
        app.world_mut().get_mut::<Sprite>(entity).unwrap().color = Color::BLACK;
        app.world_mut().entity_mut(entity).remove::<AsepriteImage>();
        app.update();
        let sprite = app.world().get::<Sprite>(entity).unwrap();
        assert!(
            SpriteFields::capture(sprite, *app.world().get::<Anchor>(entity).unwrap()) == fields
        );
        assert_eq!(sprite.color, Color::BLACK);
    }
}
