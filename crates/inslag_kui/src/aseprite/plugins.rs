use super::*;

/// Register Aseprite CSS properties. Add after [`bevy_flair::FlairPlugin`].
pub struct KuiAsepritePlugin;

impl Plugin for KuiAsepritePlugin {
    fn build(&self, app: &mut App) {
        use bevy::ui::UiSystems;
        use bevy_aseprite_ultra::prelude::AsepriteUltraPlugin;
        use bevy_flair::style::StyleSystems;
        if !app.is_plugin_added::<AsepriteUltraPlugin>() {
            app.add_plugins(AsepriteUltraPlugin);
        }
        app.init_resource::<KuiAsepriteSettings>();
        app.register_type::<world::SpriteStyle>()
            .register_component_properties::<world::SpriteStyle>();
        app.world()
            .resource::<AppTypeRegistry>()
            .write()
            .get_mut(TypeId::of::<Option<Vec2>>())
            .expect("SpriteStyle registers its size type")
            .insert(ReflectParseCss(|parser| {
                parse_property_value_with(parser, world::parse_size)
                    .map(|value| value.map(ReflectValue::new))
            }));
        let css = app.world().resource::<CssPropertyRegistry>();
        for (property, field) in [
            ("-aseprite-scale", "scale"),
            ("-aseprite-pivot-offset", "pivot_offset"),
            ("-aseprite-crop", "crop"),
            ("-aseprite-size", "size"),
        ] {
            css.register_property(property, world::SpriteStyle::property_field_ref(field));
        }
        app.register_type::<AsepriteImage>()
            .register_type::<AsepriteImagePlaceholder>()
            .register_type::<AssetPathPlaceholder<Aseprite>>()
            .register_component_properties::<AsepriteImage>();
        let parser =
            ReflectParseCss(|parser| parse_property_value_with(parser, parse_aseprite_image));
        let mut type_registry = app.world().resource::<AppTypeRegistry>().write();
        type_registry
            .get_mut(TypeId::of::<AsepriteImage>())
            .expect("AsepriteImage was just registered")
            .insert(parser);
        drop(type_registry);
        app.world()
            .resource::<CssPropertyRegistry>()
            .register_property("-aseprite-image", AsepriteImage::self_reference());
        app.add_systems(
            PostUpdate,
            (
                apply_aseprite_sampling,
                prepare_aseprite_image_nodes,
                apply_aseprite_images,
                cleanup_removed_aseprite_images,
                world::prepare,
                world::apply,
                world::cleanup,
            )
                .chain()
                .after(StyleSystems::ApplyComputedProperties)
                .before(bevy::camera::visibility::VisibilitySystems::CalculateBounds)
                .before(UiSystems::Content),
        );
    }
}
