//! Use Aseprite slices in layout images and world sprites.
//!
//! Enable the `aseprite` feature and add [`KuiAsepritePlugin`] after Flair, then
//! select an asset and slice in CSS:
//!
//! ```css
//! .machine {
//!     -aseprite-image: url("sprites/machine.aseprite") body;
//!     -aseprite-scale: 2;
//! }
//! ```
//!
//! Slices with nine-patch metadata use sliced rendering. Adjust atlas filtering
//! with [`KuiAsepriteSettings`].
mod plugins;
pub use plugins::KuiAsepritePlugin;

use std::any::TypeId;

mod world;

use bevy::{asset::LoadState, image::ImageSampler, prelude::*};
use bevy_aseprite_ultra::prelude::{Aseprite, NinePatchBehavior, RenderSlice, SliceMeta};
use bevy_flair::{
    core::{
        CssPropertyRegistry, ReflectStructPropertyRefExt, ReflectValue,
        RegisterComponentPropertiesExt,
    },
    parser::{CssError, Parser, ReflectParseCss, parse_property_value_with},
    prelude::ComponentProperties,
    style::placeholder::{
        AssetPathPlaceholder, Placeholder, ReflectPlaceholder, ResolvePlaceholderContext,
    },
};

/// Atlas filtering for Aseprite images and sprites.
///
/// Defaults to nearest-neighbor filtering. Insert before adding [`KuiAsepritePlugin`]
/// or update the resource at runtime. All slices sharing a texture use its sampler.
///
/// ```
/// use bevy::{image::ImageSampler, prelude::*};
/// use inslag_kui::aseprite::KuiAsepriteSettings;
///
/// let mut app = App::new();
/// app.insert_resource(KuiAsepriteSettings { sampler: ImageSampler::linear() });
/// ```
#[derive(Resource, Clone, Debug)]
pub struct KuiAsepriteSettings {
    pub sampler: ImageSampler,
}

impl Default for KuiAsepriteSettings {
    fn default() -> Self {
        Self {
            sampler: ImageSampler::nearest(),
        }
    }
}

fn apply_aseprite_sampling(
    settings: Res<KuiAsepriteSettings>,
    sources: Query<&AsepriteImage>,
    aseprites: Res<Assets<Aseprite>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Ultra 0.10 sets the sampler on intermediate frames, but its atlas builder
    // drops it. Configure the final texture instead. Checking current sources
    // also covers late loads, new references, settings changes, and hot reload.
    for source in &sources {
        let Some(artwork) = aseprites.get(&source.aseprite) else {
            continue;
        };
        if images
            .get(&artwork.atlas_image)
            .is_some_and(|image| image.sampler != settings.sampler)
            && let Some(mut image) = images.get_mut(&artwork.atlas_image)
        {
            image.sampler = settings.sampler.clone();
        }
    }
}

#[derive(Component, ComponentProperties, Reflect, Clone, Debug, Default, PartialEq)]
#[properties(opaque, auto_insert_remove)]
#[reflect(Component, Clone, Debug, Default, PartialEq)]
pub(crate) struct AsepriteImage {
    pub aseprite: Handle<Aseprite>,
    pub slice: String,
}

#[derive(Reflect, Clone, Debug, PartialEq)]
#[reflect(Clone, Debug, PartialEq, Placeholder)]
struct AsepriteImagePlaceholder {
    aseprite: AssetPathPlaceholder<Aseprite>,
    slice: String,
}

impl Placeholder for AsepriteImagePlaceholder {
    type Error = bevy::asset::ParseAssetPathError;
    type ResolvedValue = AsepriteImage;

    fn resolve_placeholder(
        &self,
        context: &mut ResolvePlaceholderContext,
    ) -> Result<Option<Self::ResolvedValue>, Self::Error> {
        let Some(aseprite) = self.aseprite.resolve_placeholder(context)? else {
            return Ok(None);
        };
        Ok(Some(AsepriteImage {
            aseprite,
            slice: self.slice.clone(),
        }))
    }
}

fn parse_aseprite_image(parser: &mut Parser) -> Result<ReflectValue, CssError> {
    let path = parser.expect_url_or_string()?;
    let slice = parser.expect_ident_or_string()?;
    Ok(ReflectValue::new(AsepriteImagePlaceholder {
        aseprite: AssetPathPlaceholder::new(path.as_ref()),
        slice: slice.to_string(),
    }))
}

#[derive(Clone, Debug, PartialEq)]
struct ImageFields {
    image: Handle<Image>,
    texture_atlas: Option<TextureAtlas>,
    rect: Option<Rect>,
    image_mode: NodeImageMode,
}

impl ImageFields {
    fn capture(image: &ImageNode) -> Self {
        Self {
            image: image.image.clone(),
            texture_atlas: image.texture_atlas.clone(),
            rect: image.rect,
            image_mode: image.image_mode.clone(),
        }
    }

    fn restore(&self, image: &mut ImageNode) {
        image.image = self.image.clone();
        image.texture_atlas = self.texture_atlas.clone();
        image.rect = self.rect;
        image.image_mode = self.image_mode.clone();
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AsepriteImageStatus {
    #[default]
    Pending,
    Loading,
    Ready,
    MissingSlice,
    InvalidNinePatch,
    InvalidLayout,
    Failed,
}

#[derive(Component)]
pub(crate) struct AppliedAsepriteImage {
    baseline: ImageFields,
    last_written: Option<ImageFields>,
    created_image_node: bool,
    status: AsepriteImageStatus,
}

pub(crate) fn prepare_aseprite_image_nodes(
    mut commands: Commands,
    images: Query<
        (Entity, Option<&ImageNode>),
        (
            With<AsepriteImage>,
            Without<AppliedAsepriteImage>,
            Without<crate::motion::WorldPresentation>,
        ),
    >,
) {
    for (entity, image) in &images {
        let created_image_node = image.is_none();
        let baseline = image.cloned().unwrap_or_default();
        let mut entity_commands = commands.entity(entity);
        if created_image_node {
            entity_commands.insert(baseline.clone());
        }
        entity_commands.insert(AppliedAsepriteImage {
            baseline: ImageFields::capture(&baseline),
            last_written: None,
            created_image_node,
            status: AsepriteImageStatus::Pending,
        });
    }
}

pub(crate) fn apply_aseprite_images(
    aseprites: Res<Assets<Aseprite>>,
    asset_server: Res<AssetServer>,
    mut images: Query<(
        Entity,
        Ref<AsepriteImage>,
        &mut ImageNode,
        &mut AppliedAsepriteImage,
        Option<&Name>,
    )>,
) {
    let assets_changed = aseprites.is_changed();
    for (entity, source, mut image, mut applied, name) in &mut images {
        if source.is_changed() {
            applied.status = AsepriteImageStatus::Pending;
        }
        if !assets_changed
            && !source.is_changed()
            && !matches!(
                applied.status,
                AsepriteImageStatus::Pending | AsepriteImageStatus::Loading
            )
        {
            continue;
        }

        let Some(aseprite) = aseprites.get(&source.aseprite) else {
            clear_aseprite_fields(&mut image, &mut applied);
            let new_status = match asset_server.load_state(source.aseprite.id()) {
                LoadState::Failed(error) => {
                    if applied.status != AsepriteImageStatus::Failed {
                        error!(
                            "could not load Aseprite UI image for {}: {error}",
                            entity_label(entity, name)
                        );
                    }
                    AsepriteImageStatus::Failed
                }
                _ => AsepriteImageStatus::Loading,
            };
            applied.status = new_status;
            continue;
        };

        let Some(slice) = aseprite.slices.get(&source.slice) else {
            clear_aseprite_fields(&mut image, &mut applied);
            if applied.status != AsepriteImageStatus::MissingSlice {
                error!(
                    "Aseprite UI image for {} references missing slice `{}`",
                    entity_label(entity, name),
                    source.slice
                );
            }
            applied.status = AsepriteImageStatus::MissingSlice;
            continue;
        };

        if render_aseprite_slice(&mut image, aseprite, slice).is_none() {
            clear_aseprite_fields(&mut image, &mut applied);
            if applied.status != AsepriteImageStatus::InvalidNinePatch {
                error!(
                    "Aseprite UI image for {} has invalid nine-patch metadata on slice `{}`",
                    entity_label(entity, name),
                    source.slice
                );
            }
            applied.status = AsepriteImageStatus::InvalidNinePatch;
            continue;
        }
        applied.last_written = Some(ImageFields::capture(&image));
        applied.status = AsepriteImageStatus::Ready;
    }
}

fn render_aseprite_slice(
    image: &mut ImageNode,
    aseprite: &Aseprite,
    slice: &SliceMeta,
) -> Option<()> {
    let image_mode = match slice.nine_patch {
        Some(center) => NodeImageMode::Sliced(texture_slicer(slice, center)?),
        None => NodeImageMode::Auto,
    };
    image.render_slice(aseprite, slice, NinePatchBehavior::Disabled, &mut ());
    image.rect = None;
    image.image_mode = image_mode;
    Some(())
}

fn texture_slicer(slice: &SliceMeta, center: Vec4) -> Option<TextureSlicer> {
    let size = slice.rect.size();
    let right = size.x - center.x - center.z;
    let bottom = size.y - center.y - center.w;
    let values = [center.x, right, center.y, bottom];
    if !size.is_finite()
        || size.min_element() <= 0.0
        || !center.is_finite()
        || center.z <= 0.0
        || center.w <= 0.0
        || values
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
    {
        return None;
    }
    Some(TextureSlicer {
        border: values.into(),
        center_scale_mode: SliceScaleMode::Stretch,
        sides_scale_mode: SliceScaleMode::Stretch,
        max_corner_scale: 1.0,
    })
}

fn clear_aseprite_fields(image: &mut ImageNode, applied: &mut AppliedAsepriteImage) {
    let default = ImageNode::default();
    image.image = default.image;
    image.texture_atlas = None;
    image.rect = None;
    image.image_mode = NodeImageMode::Auto;
    applied.last_written = Some(ImageFields::capture(image));
}

pub(crate) fn cleanup_removed_aseprite_images(
    mut commands: Commands,
    mut removed: RemovedComponents<AsepriteImage>,
    mut images: Query<(&mut ImageNode, &AppliedAsepriteImage)>,
) {
    for entity in removed.read() {
        let Ok((mut image, applied)) = images.get_mut(entity) else {
            continue;
        };
        if applied
            .last_written
            .as_ref()
            .is_some_and(|last| *last != ImageFields::capture(&image))
        {
            commands.entity(entity).remove::<AppliedAsepriteImage>();
            continue;
        }
        if applied.created_image_node {
            commands
                .entity(entity)
                .remove::<(ImageNode, AppliedAsepriteImage)>();
        } else {
            applied.baseline.restore(&mut image);
            commands.entity(entity).remove::<AppliedAsepriteImage>();
        }
    }
}

fn entity_label(entity: Entity, name: Option<&Name>) -> String {
    name.map_or_else(|| entity.to_string(), |name| format!("`{name}` ({entity})"))
}

#[cfg(test)]
mod tests {
    use bevy::{asset::uuid_handle, image::TextureAtlasLayout, sprite::BorderRect};
    use bevy_flair::parser::cssparser::ParserInput;

    use super::*;

    const IMAGE: Handle<Image> = uuid_handle!("83890c42-8af4-4a31-83d5-bdb3200cbf20");
    const LAYOUT: Handle<TextureAtlasLayout> = uuid_handle!("f26b0830-08f5-424c-a156-dfff1649d29f");

    #[test]
    fn sampling_is_configurable_for_shared_ui_and_world_atlases_and_reloads() {
        let mut app = App::new();
        app.init_resource::<Assets<Aseprite>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<KuiAsepriteSettings>()
            .add_systems(Update, apply_aseprite_sampling);
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let unrelated = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let mut artwork = aseprite();
        artwork.atlas_image = image.clone();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<Aseprite>>()
            .add(artwork);
        let mut other_artwork = aseprite();
        other_artwork.atlas_image = unrelated.clone();
        let _other = app
            .world_mut()
            .resource_mut::<Assets<Aseprite>>()
            .add(other_artwork);
        for world in [false, true] {
            let mut entity = app.world_mut().spawn(AsepriteImage {
                aseprite: handle.clone(),
                slice: "part".into(),
            });
            if world {
                entity.insert(crate::motion::WorldPresentation::default());
            }
        }
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&image)
                .unwrap()
                .sampler,
            ImageSampler::nearest()
        );
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&unrelated)
                .unwrap()
                .sampler,
            ImageSampler::Default
        );

        app.world_mut()
            .resource_mut::<KuiAsepriteSettings>()
            .sampler = ImageSampler::linear();
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&image)
                .unwrap()
                .sampler,
            ImageSampler::linear()
        );

        // A reload replaces the atlas handle; the current configuration follows it.
        let replacement = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        app.world_mut()
            .resource_mut::<Assets<Aseprite>>()
            .get_mut(&handle)
            .unwrap()
            .atlas_image = replacement.clone();
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&replacement)
                .unwrap()
                .sampler,
            ImageSampler::linear()
        );
    }

    #[test]
    fn parses_asset_and_slice_as_one_value() {
        let mut input = ParserInput::new(r#"url("ui/textures/frames.aseprite") panel_fern01"#);
        let mut parser = Parser::new(&mut input);

        let value = parser
            .parse_entirely(|parser| {
                parse_aseprite_image(parser).map_err(CssError::into_parse_error)
            })
            .unwrap();
        let placeholder = value
            .downcast_value_ref::<AsepriteImagePlaceholder>()
            .unwrap();

        assert_eq!(placeholder.slice, "panel_fern01");
        assert!(format!("{:?}", placeholder.aseprite).contains("ui/textures/frames.aseprite"));
    }

    #[test]
    fn parser_requires_exactly_one_slice_name() {
        for value in [
            r#"url("ui/textures/frames.aseprite")"#,
            r#"url("ui/textures/frames.aseprite") panel_fern01 extra"#,
        ] {
            let mut input = ParserInput::new(value);
            let mut parser = Parser::new(&mut input);
            assert!(
                parser
                    .parse_entirely(
                        |parser| parse_aseprite_image(parser).map_err(CssError::into_parse_error)
                    )
                    .is_err(),
                "accepted invalid value `{value}`"
            );
        }
    }

    #[test]
    fn renders_asymmetric_native_nine_patch() {
        let aseprite = aseprite();
        let slice = slice(Some(Vec4::new(71.0, 82.0, 95.0, 36.0)));
        let mut image = ImageNode::default();

        assert_eq!(
            render_aseprite_slice(&mut image, &aseprite, &slice),
            Some(())
        );
        assert_eq!(image.image, IMAGE);
        assert_eq!(
            image.texture_atlas,
            Some(TextureAtlas {
                layout: LAYOUT,
                index: 7,
            })
        );
        assert_eq!(
            image.image_mode,
            NodeImageMode::Sliced(TextureSlicer {
                border: BorderRect::from([71.0, 61.0, 82.0, 80.0]),
                center_scale_mode: SliceScaleMode::Stretch,
                sides_scale_mode: SliceScaleMode::Stretch,
                max_corner_scale: 1.0,
            })
        );
    }

    #[test]
    fn renders_slice_without_center_as_normal_image() {
        let aseprite = aseprite();
        let slice = slice(None);
        let mut image = ImageNode {
            image_mode: NodeImageMode::Stretch,
            ..Default::default()
        };

        assert_eq!(
            render_aseprite_slice(&mut image, &aseprite, &slice),
            Some(())
        );
        assert_eq!(image.image_mode, NodeImageMode::Auto);
    }

    #[test]
    fn rejects_center_outside_slice_without_mutating_image() {
        let aseprite = aseprite();
        let slice = slice(Some(Vec4::new(200.0, 82.0, 95.0, 36.0)));
        let mut image = ImageNode::default();
        let before = ImageFields::capture(&image);

        assert_eq!(render_aseprite_slice(&mut image, &aseprite, &slice), None);
        assert_eq!(ImageFields::capture(&image), before);
    }

    #[test]
    fn applies_reloaded_metadata_and_cleans_up_managed_image_node() {
        let mut app = crate::runtime::test_support::app(&[]);
        app.init_asset::<Aseprite>().add_systems(
            Update,
            (
                prepare_aseprite_image_nodes,
                apply_aseprite_images,
                cleanup_removed_aseprite_images,
            )
                .chain(),
        );

        let mut asset = aseprite();
        asset.slices.insert(
            "panel".to_owned(),
            slice(Some(Vec4::new(71.0, 82.0, 95.0, 36.0))),
        );
        let handle = app
            .world_mut()
            .resource_mut::<Assets<Aseprite>>()
            .add(asset);
        let entity = app
            .world_mut()
            .spawn((
                Node::default(),
                AsepriteImage {
                    aseprite: handle.clone(),
                    slice: "panel".to_owned(),
                },
            ))
            .id();

        app.update();

        let image = app.world().get::<ImageNode>(entity).unwrap();
        assert_eq!(image.image, IMAGE);
        let NodeImageMode::Sliced(slicer) = &image.image_mode else {
            panic!("expected native nine-patch mode");
        };
        assert_eq!(slicer.border, BorderRect::from([71.0, 61.0, 82.0, 80.0]));

        app.world_mut()
            .resource_mut::<Assets<Aseprite>>()
            .get_mut(&handle)
            .unwrap()
            .slices
            .get_mut("panel")
            .unwrap()
            .nine_patch = Some(Vec4::new(25.0, 23.0, 177.0, 151.0));
        app.update();

        let image = app.world().get::<ImageNode>(entity).unwrap();
        let NodeImageMode::Sliced(slicer) = &image.image_mode else {
            panic!("expected reloaded native nine-patch mode");
        };
        assert_eq!(slicer.border, BorderRect::from([25.0, 25.0, 23.0, 24.0]));

        app.world_mut().entity_mut(entity).remove::<AsepriteImage>();
        app.update();

        assert!(app.world().get::<ImageNode>(entity).is_none());
        assert!(app.world().get::<AppliedAsepriteImage>(entity).is_none());
    }

    fn aseprite() -> Aseprite {
        let mut aseprite = Aseprite::default();
        aseprite.atlas_layout = LAYOUT;
        aseprite.atlas_image = IMAGE;
        aseprite
    }

    fn slice(nine_patch: Option<Vec4>) -> SliceMeta {
        SliceMeta {
            rect: Rect::new(21.0, 18.0, 248.0, 216.0),
            atlas_id: 7,
            pivot: None,
            nine_patch,
            keys: Vec::new(),
        }
    }
}
