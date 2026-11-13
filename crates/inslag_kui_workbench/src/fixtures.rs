mod plugins;
pub(crate) use plugins::FixturePlugin;

use std::path::Path;

use bevy::{
    asset::io::{
        AssetReader, AssetReaderError, PathStream, Reader, file::FileAssetReader,
        memory::MemoryAssetReader,
    },
    prelude::*,
};
use inslag_state_tree::prelude::*;

use crate::{ToolAction, data::Data, xml};

pub(crate) struct SourceReader {
    pub memory: MemoryAssetReader,
    pub files: FileAssetReader,
}

impl SourceReader {
    fn generated(path: &Path) -> bool {
        path.starts_with("__kui_workbench")
    }
}

impl AssetReader for SourceReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let reader: Box<dyn Reader> = if Self::generated(path) {
            Box::new(self.memory.read(path).await?)
        } else {
            Box::new(self.files.read(path).await?)
        };
        Ok(reader)
    }
    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let reader: Box<dyn Reader> = if Self::generated(path) {
            Box::new(self.memory.read_meta(path).await?)
        } else {
            Box::new(self.files.read_meta(path).await?)
        };
        Ok(reader)
    }
    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        if Self::generated(path) {
            self.memory.read_directory(path).await
        } else {
            self.files.read_directory(path).await
        }
    }
    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        if Self::generated(path) {
            self.memory.is_directory(path).await
        } else {
            self.files.is_directory(path).await
        }
    }
}

#[derive(EntityEvent)]
pub(crate) struct FixtureChanged {
    pub entity: Entity,
    pub data: Data,
}
impl StateTreeEvent for FixtureChanged {
    const AUTHORED_NAME: &'static str = "workbench.fixture";
    fn event_data(&self) -> Option<LuauValue> {
        Some(LuauValue::Record(self.data.clone()))
    }
}
impl EntityStateTreeEvent for FixtureChanged {}

pub(crate) fn install(
    world: &mut World,
    controller: Entity,
    src: &str,
    data: &Data,
) -> Result<(), String> {
    let definition = definition(src, data)?;
    let overrides =
        StateTreeDataOverrides::try_new(data.clone()).map_err(|error| error.to_string())?;
    let handle = world
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition);
    world
        .entity_mut(controller)
        .insert(StateTreeInstance::new_with_initial_data(handle, overrides));
    Ok(())
}

pub(crate) fn definition(src: &str, data: &Data) -> Result<StateTreeDefinition, String> {
    if data.contains_key("_workbench_render_status") {
        return Err("_workbench_render_status is reserved by the preview controller".into());
    }
    let declarations = data
        .keys()
        .map(|key| format!("<data id=\"{}\"/>", xml(key)))
        .collect::<String>();
    let assignments = data
        .keys()
        .map(|key| {
            format!(
                "<assign location=\"{}\" expr=\"_event.data[{}]\"/>",
                xml(key),
                xml(&format!("{key:?}"))
            )
        })
        .collect::<String>();
    let source = format!(
        r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:bst="urn:inslag-state-tree" datamodel="luau" initial="active">
      <datamodel>{declarations}<data id="_workbench_render_status" expr="'Loading'"/></datamodel>
      <state id="active">
        <bst:activity id="view" name="ui.render"><bst:arg name="src" expr="{}"/></bst:activity>
        <transition event="workbench.fixture">{assignments}</transition>
        <transition event="ui.render.ready"><assign location="_workbench_render_status" expr="'Ready'"/></transition>
        <transition event="error.activity.view"><assign location="_workbench_render_status" expr="_event.data"/></transition>
        <transition event="ui.action"><bst:effect name="workbench.command">
          <bst:arg name="action" expr="_event.data.action"/><bst:arg name="value" expr="_event.data.value"/>
          <bst:arg name="position" expr="_event.data.position"/><bst:arg name="start" expr="_event.data.start"/>
        </bst:effect></transition>
      </state></scxml>"#,
        xml(&format!("{src:?}"))
    );
    let parsed = inslag_state_tree::scxml::parse("workbench-preview.scxml", &source)
        .map_err(|error| error.to_string())?;
    inslag_state_tree::scxml::compile(&parsed).map_err(|error| error.to_string())
}

pub(crate) fn update(world: &mut World, controller: Entity, data: Data) {
    use bevy::ecs::system::RunSystemOnce;
    world
        .run_system_once(move |mut events: EntityStateTreeEventWriter| {
            events.write(FixtureChanged {
                entity: controller,
                data: data.clone(),
            });
        })
        .expect("fixture event writer");
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        asset::io::{AssetSourceBuilder, AssetSourceId, memory::Dir},
        camera::NormalizedRenderTarget,
        picking::{
            backend::HitData,
            events::Pointer,
            pointer::{Location, PointerId},
        },
    };

    #[test]
    fn previews_render_update_and_deliver_domain_effects_through_scxml() {
        let directory = Dir::default();
        directory.insert_asset_text(Path::new("fixture.kui"), r#"<view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="fixture"><button id="choice" on-click="choose" bind:value="id"><text bind:text="title"/></button></view>"#);
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
            MinimalPlugins,
            AssetPlugin {
                watch_for_changes_override: Some(false),
                ..default()
            },
        ));
        app.init_asset::<Image>().add_plugins((
            StateTreePlugin::new(Update),
            FixturePlugin,
            inslag_kui::state_tree::KuiStateTreePlugin,
        ));
        let controller = app.world_mut().spawn_empty().id();
        let mut data = Data::from([
            ("title".into(), LuauValue::String("First".into())),
            ("id".into(), LuauValue::String("oak".into())),
        ]);
        install(app.world_mut(), controller, "fixture.kui", &data).unwrap();
        app.finish();
        app.cleanup();
        for _ in 0..512 {
            app.update();
            if app
                .world()
                .get::<StateTreeInstance>(controller)
                .unwrap()
                .data("_workbench_render_status")
                == Some(&LuauValue::String("Ready".into()))
            {
                break;
            }
            std::thread::yield_now();
        }
        assert_eq!(
            app.world()
                .get::<StateTreeInstance>(controller)
                .unwrap()
                .data("_workbench_render_status"),
            Some(&LuauValue::String("Ready".into()))
        );
        data.insert("title".into(), LuauValue::String("Updated".into()));
        data.insert("id".into(), LuauValue::String("ash".into()));
        update(app.world_mut(), controller, data);
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&Text>()
                .single(app.world())
                .unwrap()
                .0,
            "Updated"
        );
        let button = app
            .world_mut()
            .query_filtered::<Entity, With<bevy::ui_widgets::Button>>()
            .single(app.world())
            .unwrap();
        app.world_mut().trigger(PointerClick {
            entity: button,
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
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0., None, None),
            duration: std::time::Duration::ZERO,
            count: 1,
        });
        app.world_mut().flush();
        app.update();
        let commands: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<ToolAction>>()
            .drain()
            .collect();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].view, controller);
        assert_eq!(commands[0].action, "choose");
        assert_eq!(commands[0].value.as_deref(), Some("ash"));
        let mut ancestor = button;
        while ancestor != controller {
            ancestor = app.world().get::<ChildOf>(ancestor).unwrap().parent();
        }
        assert_eq!(app.world().get::<Children>(controller).unwrap().len(), 1);
    }

    #[test]
    fn invalid_fixture_roots_are_errors_not_renderer_escape_hatches_or_panics() {
        assert!(
            definition(
                "fixture.kui",
                &Data::from([("bad root".into(), LuauValue::Nil)])
            )
            .is_err()
        );
        assert!(
            definition(
                "fixture.kui",
                &Data::from([("_workbench_render_status".into(), LuauValue::Nil)])
            )
            .is_err()
        );
    }
}
