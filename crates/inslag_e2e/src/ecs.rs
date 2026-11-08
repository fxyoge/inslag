//! Generic ECS reflection and authored state-tree adapters. NEVER add a scenario,
//! assertion, selector policy, or game-specific fixture to this Rust module.
use bevy::{
    ecs::{component::ComponentId, reflect::ReflectComponent},
    prelude::*,
    reflect::{TypeRegistration, TypeRegistry, serde::TypedReflectSerializer},
};
use inslag_state_tree::prelude::*;
use mlua::{Lua, LuaSerdeExt, Table, Value};

pub(crate) fn query(world: &mut World, lua: &Lua, args: &Table) -> Result<Value, String> {
    let with: Vec<String> = args
        .get::<Option<Vec<String>>>("with")
        .map_err(err)?
        .unwrap_or_default();
    let without: Vec<String> = args
        .get::<Option<Vec<String>>>("without")
        .map_err(err)?
        .unwrap_or_default();
    let with = with
        .iter()
        .map(|n| component_id(world, n))
        .collect::<Result<Vec<_>, _>>()?;
    let without = without
        .iter()
        .map(|n| component_id(world, n))
        .collect::<Result<Vec<_>, _>>()?;
    let name: Option<String> = args.get("name").map_err(err)?;
    let owner = args
        .get::<Option<String>>("owner")
        .map_err(err)?
        .map(|s| entity(&s))
        .transpose()?;
    let mut builder = bevy::ecs::query::QueryBuilder::<Entity>::new(world);
    for id in with {
        builder.with_id(id);
    }
    for id in without {
        builder.without_id(id);
    }
    let mut query = builder.build();
    let mut ids: Vec<_> = query
        .iter(world)
        .filter(|id| {
            name.as_ref()
                .is_none_or(|name| world.get::<Name>(*id).is_some_and(|n| n.as_str() == name))
                && owner.is_none_or(|owner| owned_by(world, *id, owner))
        })
        .collect();
    ids.sort_by_key(|e| e.to_bits());
    if ids.len() > 10000 {
        return Err("query exceeds 10000 entities; narrow the query".into());
    }
    let rows = lua.create_table().map_err(err)?;
    for (index, id) in ids.into_iter().enumerate() {
        let row = lua.create_table().map_err(err)?;
        row.set("id", id.to_bits().to_string()).map_err(err)?;
        if let Some(name) = world.get::<Name>(id) {
            row.set("name", name.as_str()).map_err(err)?;
        }
        rows.raw_set(index + 1, row).map_err(err)?;
    }
    Ok(Value::Table(rows))
}

pub(crate) fn read(
    world: &mut World,
    lua: &Lua,
    args: &Table,
    resource: bool,
) -> Result<Value, String> {
    let name: String = args.get("type").map_err(err)?;
    let id = component_id(world, &name)?;
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let registration = registration(&registry, &name)?;
    let target = if resource {
        world
            .resource_entities()
            .get(id)
            .ok_or_else(|| format!("missing resource {name}"))?
    } else {
        entity(&args.get::<String>("entity").map_err(err)?)?
    };
    let value = world
        .get_reflect(target, registration.type_id())
        .map_err(err)?;
    reflect_to_lua(lua, value.as_partial_reflect(), &registry, 0)
}

/// Preserve complex map keys and entity precision instead of forcing the entire
/// ECS value through JSON (which cannot represent e.g. an IVec2 map key).
pub(crate) fn write(
    world: &mut World,
    lua: &Lua,
    args: &Table,
    resource: bool,
) -> Result<(), String> {
    let name: String = args.get("type").map_err(err)?;
    let id = component_id(world, &name)?;
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let registration = registration(&registry, &name)?;
    let value = super::construct::decode(
        lua,
        args.get("value").map_err(err)?,
        registration,
        &registry,
        0,
    )?;
    let component = registration
        .data::<ReflectComponent>()
        .ok_or("type lacks ReflectComponent")?;
    if resource {
        if registration
            .data::<bevy::ecs::reflect::ReflectResource>()
            .is_none()
        {
            return Err(format!("{name} is not a resource"));
        }
        world.insert_reflect_resource(id, value);
    } else {
        let target = entity(&args.get::<String>("entity").map_err(err)?)?;
        let mut target = world.get_entity_mut(target).map_err(err)?;
        component.insert(&mut target, value.as_ref(), &registry);
    }
    Ok(())
}

/// Replace a requested asset in memory, keeping all existing handle references.
/// Returning false lets the Luau setup helper await its asynchronous load.
pub(crate) fn write_asset(world: &mut World, lua: &Lua, args: &Table) -> Result<bool, String> {
    let path: String = args.get("path").map_err(err)?;
    inslag_kui::source::validate_asset_path(&path).map_err(str::to_owned)?;
    let server = world.resource::<AssetServer>();
    let ids = server.get_path_ids(path.clone());
    let id = match ids.as_slice() {
        [id] => *id,
        [] => return Err(format!("asset {path} has not been requested")),
        _ => return Err(format!("asset path {path} has multiple asset types")),
    };
    if let Some(bevy::asset::LoadState::Failed(error)) = server.get_load_state(id) {
        return Err(format!("asset {path} failed to load: {error}"));
    }
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let registration = registry
        .get(id.type_id())
        .ok_or("unregistered asset type")?;
    let asset = registration
        .data::<bevy::asset::ReflectAsset>()
        .ok_or("asset type lacks ReflectAsset")?;
    if asset.get(world, id).is_none() {
        return Ok(false);
    }
    let value = super::construct::decode(
        lua,
        args.get("value").map_err(err)?,
        registration,
        &registry,
        0,
    )?;
    asset.insert(world, id, value.as_ref()).map_err(err)?;
    Ok(true)
}

#[derive(bevy::prelude::Resource, Default)]
struct FixtureAssets(
    std::collections::BTreeMap<String, bevy::asset::Handle<bevy::asset::LoadedUntypedAsset>>,
);

/// Load an ordinary authored fixture and copy it over an already requested asset.
/// Retain the source handle so repeated asynchronous setup requests share a load.
pub(crate) fn copy_asset(world: &mut World, args: &Table) -> Result<bool, String> {
    let path: String = args.get("path").map_err(err)?;
    let source: String = args.get("source").map_err(err)?;
    inslag_kui::source::validate_asset_path(&path).map_err(str::to_owned)?;
    inslag_kui::source::validate_asset_path(&source).map_err(str::to_owned)?;
    world.init_resource::<FixtureAssets>();
    let handle = if let Some(handle) = world.resource::<FixtureAssets>().0.get(&source) {
        handle.clone()
    } else {
        let handle = world
            .resource::<AssetServer>()
            .load_builder()
            .load_untyped(source.clone());
        world
            .resource_mut::<FixtureAssets>()
            .0
            .insert(source, handle.clone());
        handle
    };
    let server = world.resource::<AssetServer>();
    if let Some(bevy::asset::LoadState::Failed(error)) = server.get_load_state(handle.id()) {
        return Err(error.to_string());
    }
    let Some(loaded) = world
        .resource::<Assets<bevy::asset::LoadedUntypedAsset>>()
        .get(&handle)
    else {
        return Ok(false);
    };
    let source = loaded.handle.id();
    let ids = server.get_path_ids(path.clone());
    let [destination] = ids.as_slice() else {
        return Err(format!("asset {path} must already be requested"));
    };
    if destination.type_id() != source.type_id() {
        return Err("fixture asset types must match".into());
    }
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let asset = registry
        .get(source.type_id())
        .and_then(|r| r.data::<bevy::asset::ReflectAsset>())
        .ok_or("fixture type lacks ReflectAsset")?;
    let Some(value) = asset.get(world, source) else {
        return Ok(false);
    };
    let value = value.reflect_clone().map_err(err)?;
    asset
        .insert(world, *destination, value.as_partial_reflect())
        .map_err(err)?;
    Ok(true)
}

pub(crate) fn remove(world: &mut World, args: &Table) -> Result<(), String> {
    let name: String = args.get("type").map_err(err)?;
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let component = registration(&registry, &name)?
        .data::<ReflectComponent>()
        .ok_or("type lacks ReflectComponent")?;
    let target = entity(&args.get::<String>("entity").map_err(err)?)?;
    component.remove(&mut world.get_entity_mut(target).map_err(err)?);
    Ok(())
}

pub(crate) fn trees_ready(world: &mut World) -> Result<bool, String> {
    let mut ready = true;
    for state in world.query::<&StateTreeInstance>().iter(world) {
        if let Some(bevy::asset::LoadState::Failed(error)) = world
            .resource::<AssetServer>()
            .get_load_state(state.definition_id())
        {
            return Err(format!("state tree definition failed to load: {error}"));
        }
        match state.status() {
            InstanceStatus::Pending => ready = false,
            InstanceStatus::Faulted(error) => {
                return Err(format!("state tree fault: {error:?}"));
            }
            _ => {}
        }
    }
    if let Some(failure) = world.query::<&StateTreeLinkFailure>().iter(world).next() {
        return Err(format!("state tree link failure: {:?}", failure.error()));
    }
    Ok(ready)
}

#[cfg_attr(
    dylint_lib = "inslag_state_tree_private_access",
    allow(
        inslag_state_tree_private_access,
        reason = "failure reports expose authored state for test diagnostics, not gameplay decisions"
    )
)]
pub(crate) fn tree_snapshot(world: &mut World) -> serde_json::Value {
    serde_json::Value::Array(world.query::<(Entity, &StateTreeInstance)>().iter(world).map(|(id, state)| {
        serde_json::json!({"entity": id.to_bits().to_string(), "status": format!("{:?}", state.status()), "states": state.active_state_names().collect::<Vec<_>>()})
    }).collect())
}

#[cfg_attr(
    dylint_lib = "inslag_state_tree_private_access",
    allow(
        inslag_state_tree_private_access,
        reason = "the generic test adapter exposes statechart observations to Luau assertions"
    )
)]
pub(crate) fn tree(
    world: &mut World,
    lua: &Lua,
    args: &Table,
    create: bool,
) -> Result<Value, String> {
    let id = entity(&args.get::<String>("entity").map_err(err)?)?;
    if create {
        let asset: String = args.get("asset").map_err(err)?;
        inslag_kui::source::validate_asset_path(&asset).map_err(str::to_owned)?;
        let initial: Option<Table> = args.get("data").map_err(err)?;
        let mut values = Vec::new();
        if let Some(initial) = initial {
            for pair in initial.pairs::<String, Value>() {
                let (name, value) = pair.map_err(err)?;
                values.push((name, to_tree_value(value, 0)?));
            }
        }
        let data = StateTreeDataOverrides::try_new(values).map_err(err)?;
        if let Some(config) = args.get::<Option<Table>>("config").map_err(err)? {
            let mut values = Vec::new();
            for pair in config.pairs::<String, Value>() {
                let (name, value) = pair.map_err(err)?;
                values.push((name, to_tree_value(value, 0)?));
            }
            world
                .get_entity_mut(id)
                .map_err(err)?
                .insert(inslag_state_tree::StateTreeConfiguration::new(values));
        }
        let handle = world.resource::<AssetServer>().load(asset);
        world
            .get_entity_mut(id)
            .map_err(err)?
            .insert(StateTreeInstance::new_with_initial_data(handle, data));
        Ok(Value::Nil)
    } else {
        let state = world
            .get::<StateTreeInstance>(id)
            .ok_or("entity has no state tree")?;
        if let Some(key) = args.get::<Option<String>>("key").map_err(err)? {
            return state
                .data(&key)
                .map(|value| from_tree_value(lua, value))
                .transpose()
                .map(|v| v.unwrap_or(Value::Nil));
        }
        let result = lua.create_table().map_err(err)?;
        result
            .set("status", format!("{:?}", state.status()))
            .map_err(err)?;
        result
            .set(
                "states",
                lua.to_value(&state.active_state_names().collect::<Vec<_>>())
                    .map_err(err)?,
            )
            .map_err(err)?;
        Ok(Value::Table(result))
    }
}

pub(crate) fn screen(world: &World, lua: &Lua, args: &Table) -> Result<Value, String> {
    let id = entity(&args.get::<String>("entity").map_err(err)?)?;
    let mut cursor = id;
    let mut layout = false;
    let mut visible = true;
    for _ in 0..256 {
        if world.get::<Visibility>(cursor) == Some(&Visibility::Hidden) {
            visible = false;
        }
        layout |= world.get::<Node>(cursor).is_some();
        match world.get::<ChildOf>(cursor) {
            Some(parent) => cursor = parent.parent(),
            None => break,
        }
    }
    if !visible {
        return Err("entity is hidden".into());
    }
    let point = if let (Some(node), Some(transform)) = (
        world.get::<ComputedNode>(id),
        world.get::<UiGlobalTransform>(id),
    ) {
        if node.size().min_element() <= 0.0 {
            return Err("UI node has no layout area".into());
        }
        let (_, _, center) = transform.to_scale_angle_translation();
        center
            * node.inverse_scale_factor
            * world.get_resource::<UiScale>().map_or(1.0, |scale| scale.0)
    } else {
        if layout {
            return Err("screen UI node has no computed geometry".into());
        }
        let position = sprite_point(world, id)?;
        let window = world
            .iter_entities()
            .find_map(|e| {
                e.contains::<bevy::window::PrimaryWindow>()
                    .then(|| e.get::<Window>())
                    .flatten()
            })
            .ok_or("missing primary window")?;
        let layers = world
            .get::<bevy::camera::visibility::RenderLayers>(id)
            .cloned()
            .unwrap_or_default();
        let mut cameras: Vec<_> = world
            .iter_entities()
            .filter_map(|e| {
                let camera = e.get::<Camera>()?;
                let camera_layers = e
                    .get::<bevy::camera::visibility::RenderLayers>()
                    .cloned()
                    .unwrap_or_default();
                if !camera.is_active || !camera_layers.intersects(&layers) {
                    return None;
                }
                let point = camera
                    .world_to_viewport(e.get::<GlobalTransform>()?, position)
                    .ok()?;
                Some((camera.order, point))
            })
            .collect();
        cameras.sort_by_key(|(order, _)| *order);
        let point = cameras.last().ok_or("no camera projects entity")?.1;
        if point.x < 0.0 || point.y < 0.0 || point.x >= window.width() || point.y >= window.height()
        {
            return Err("entity is outside viewport".into());
        }
        point
    };
    let result = lua.create_table().map_err(err)?;
    result.set("x", point.x).map_err(err)?;
    result.set("y", point.y).map_err(err)?;
    Ok(Value::Table(result))
}

/// A transform group's origin need not lie on any visible pixel. Observe a
/// visible descendant's opaque image region, including crops, atlases and flips.
/// This is geometry only: picking/occlusion still decides what receives input.
fn component_id(world: &mut World, name: &str) -> Result<ComponentId, String> {
    let matches: Vec<_> = world
        .components()
        .iter_registered()
        .filter(|(_, c)| {
            let path = c.name().to_string();
            path == name || path.rsplit("::").next() == Some(name)
        })
        .map(|(id, _)| id)
        .collect();
    match matches.as_slice() {
        [id] => Ok(*id),
        [] => {
            let registry = world.resource::<AppTypeRegistry>().clone();
            let registry = registry.read();
            let component = registration(&registry, name)?
                .data::<ReflectComponent>()
                .ok_or_else(|| format!("{name} is not a reflected component"))?;
            Ok(component.register_component(world))
        }
        _ => Err(format!(
            "ambiguous component {name}; use its full type path"
        )),
    }
}

pub(crate) fn registration<'a>(
    registry: &'a TypeRegistry,
    name: &str,
) -> Result<&'a TypeRegistration, String> {
    registry
        .get_with_type_path(name)
        .or_else(|| registry.get_with_short_type_path(name))
        .ok_or_else(|| {
            format!("unknown or ambiguous reflected type {name}; use its full type path")
        })
}

pub(crate) fn entity(id: &str) -> Result<Entity, String> {
    let bits = id
        .parse::<u64>()
        .map_err(|_| format!("invalid entity ID {id}"))?;
    Entity::try_from_bits(bits).ok_or_else(|| format!("invalid entity bits {id}"))
}

fn owned_by(world: &World, mut cursor: Entity, owner: Entity) -> bool {
    for _ in 0..256 {
        if cursor == owner {
            return true;
        }
        match world.get::<ChildOf>(cursor) {
            Some(parent) => cursor = parent.parent(),
            None => return false,
        }
    }
    false
}

fn reflect_to_lua(
    lua: &Lua,
    value: &dyn bevy::reflect::PartialReflect,
    registry: &TypeRegistry,
    depth: usize,
) -> Result<Value, String> {
    use bevy::reflect::ReflectRef;
    if depth > 32 {
        return Err("ECS observation exceeds nesting limit".into());
    }
    if let Some(entity) = value.try_downcast_ref::<Entity>() {
        return Ok(Value::String(
            lua.create_string(entity.to_bits().to_string())
                .map_err(err)?,
        ));
    }
    let table = lua.create_table().map_err(err)?;
    let child = |v: &dyn bevy::reflect::PartialReflect| reflect_to_lua(lua, v, registry, depth + 1);
    match value.reflect_ref() {
        ReflectRef::Struct(value) => {
            for i in 0..value.field_len() {
                table
                    .set(
                        value.name_at(i).unwrap(),
                        child(value.field_at(i).unwrap())?,
                    )
                    .map_err(err)?;
            }
        }
        ReflectRef::TupleStruct(value) => {
            for i in 0..value.field_len() {
                table
                    .raw_set(i + 1, child(value.field(i).unwrap())?)
                    .map_err(err)?;
            }
        }
        ReflectRef::Tuple(value) => {
            for i in 0..value.field_len() {
                table
                    .raw_set(i + 1, child(value.field(i).unwrap())?)
                    .map_err(err)?;
            }
        }
        ReflectRef::List(value) => {
            if value.len() > 10000 {
                return Err("observation list exceeds 10000 entries".into());
            }
            for (i, v) in value.iter().enumerate() {
                table.raw_set(i + 1, child(v)?).map_err(err)?;
            }
        }
        ReflectRef::Array(value) => {
            if value.len() > 10000 {
                return Err("observation array exceeds 10000 entries".into());
            }
            for (i, v) in value.iter().enumerate() {
                table.raw_set(i + 1, child(v)?).map_err(err)?;
            }
        }
        ReflectRef::Map(value) => {
            if value.len() > 10000 {
                return Err("observation map exceeds 10000 entries".into());
            }
            for (i, (key, v)) in value.iter().enumerate() {
                let entry = lua.create_table().map_err(err)?;
                entry.set("key", child(key)?).map_err(err)?;
                entry.set("value", child(v)?).map_err(err)?;
                table.raw_set(i + 1, entry).map_err(err)?;
            }
        }
        ReflectRef::Set(value) => {
            if value.len() > 10000 {
                return Err("observation set exceeds 10000 entries".into());
            }
            for (i, v) in value.iter().enumerate() {
                table.raw_set(i + 1, child(v)?).map_err(err)?;
            }
        }
        ReflectRef::Enum(value) => {
            table.set("variant", value.variant_name()).map_err(err)?;
            let fields = lua.create_table().map_err(err)?;
            for i in 0..value.field_len() {
                let v = child(value.field_at(i).unwrap())?;
                if let Some(name) = value.name_at(i) {
                    fields.set(name, v).map_err(err)?;
                } else {
                    fields.raw_set(i + 1, v).map_err(err)?;
                }
            }
            table.set("fields", fields).map_err(err)?;
        }
        ReflectRef::Opaque(_) => {
            let json = serde_json::to_value(TypedReflectSerializer::new(value, registry)).map_err(
                |e| {
                    format!(
                        "cannot observe opaque type {}: {e}",
                        value.reflect_type_path()
                    )
                },
            )?;
            return lua.to_value(&json).map_err(err);
        }
    }
    Ok(Value::Table(table))
}

fn to_tree_value(value: Value, depth: usize) -> Result<LuauValue, String> {
    if depth > 32 {
        return Err("state tree initial data exceeds nesting limit".into());
    }
    Ok(match value {
        Value::Nil => LuauValue::Nil,
        Value::Boolean(value) => LuauValue::Boolean(value),
        Value::Integer(value) => LuauValue::Integer(value),
        Value::Number(value) if value.is_finite() => LuauValue::Number(value),
        Value::String(value) => LuauValue::String(value.to_str().map_err(err)?.to_string()),
        Value::Table(value) if value.raw_len() > 0 => LuauValue::Sequence(
            value
                .sequence_values::<Value>()
                .map(|v| to_tree_value(v.map_err(err)?, depth + 1))
                .collect::<Result<_, _>>()?,
        ),
        Value::Table(value) => LuauValue::Record(
            value
                .pairs::<String, Value>()
                .map(|v| {
                    let (k, v) = v.map_err(err)?;
                    Ok((k, to_tree_value(v, depth + 1)?))
                })
                .collect::<Result<_, String>>()?,
        ),
        _ => return Err("unsupported state tree initial value".into()),
    })
}

fn from_tree_value(lua: &Lua, value: &LuauValue) -> Result<Value, String> {
    match value {
        LuauValue::Nil => Ok(Value::Nil),
        LuauValue::Boolean(v) => Ok(Value::Boolean(*v)),
        LuauValue::Integer(v) => Ok(Value::Integer(*v)),
        LuauValue::Number(v) => Ok(Value::Number(*v)),
        LuauValue::String(v) => Ok(Value::String(lua.create_string(v).map_err(err)?)),
        LuauValue::Sequence(values) => {
            let table = lua.create_table().map_err(err)?;
            for (i, v) in values.iter().enumerate() {
                table
                    .raw_set(i + 1, from_tree_value(lua, v)?)
                    .map_err(err)?;
            }
            Ok(Value::Table(table))
        }
        LuauValue::Record(values) => {
            let table = lua.create_table().map_err(err)?;
            for (k, v) in values {
                table
                    .set(k.as_str(), from_tree_value(lua, v)?)
                    .map_err(err)?;
            }
            Ok(Value::Table(table))
        }
    }
}

fn sprite_point(world: &World, root: Entity) -> Result<Vec3, String> {
    let images = world.resource::<Assets<Image>>();
    let atlases = world.resource::<Assets<TextureAtlasLayout>>();
    let mut pending = vec![root];
    let mut candidates = Vec::new();
    while let Some(id) = pending.pop() {
        if let Some(children) = world.get::<Children>(id) {
            pending.extend(children.iter());
        }
        let Some(sprite) = world.get::<Sprite>(id) else {
            continue;
        };
        if !world
            .get::<InheritedVisibility>(id)
            .is_some_and(|v| v.get())
            || sprite.color.alpha() <= 0.1
        {
            continue;
        }
        let Some(transform) = world.get::<GlobalTransform>(id) else {
            continue;
        };
        if transform.affine().matrix3.determinant().abs() < 1e-8 {
            continue;
        }
        let Some(image) = images.get(&sprite.image) else {
            continue;
        };
        let atlas = sprite
            .texture_atlas
            .as_ref()
            .and_then(|a| a.texture_rect(atlases))
            .map(|r| r.as_rect());
        let rect = match (atlas, sprite.rect) {
            (Some(atlas), Some(rect)) => {
                Rect::from_corners(atlas.min + rect.min, atlas.min + rect.max)
            }
            (Some(rect), None) | (None, Some(rect)) => rect,
            _ => Rect::from_corners(Vec2::ZERO, image.size().as_vec2()),
        };
        let mut min = UVec2::splat(u32::MAX);
        let mut max = UVec2::ZERO;
        let mut opaque = Vec::new();
        for y in (rect.min.y.max(0.0) as u32)..(rect.max.y.min(image.height() as f32) as u32) {
            for x in (rect.min.x.max(0.0) as u32)..(rect.max.x.min(image.width() as f32) as u32) {
                if image.get_color_at(x, y).map_err(err)?.alpha() * sprite.color.alpha() > 0.1 {
                    let pixel = UVec2::new(x, y);
                    min = min.min(pixel);
                    max = max.max(pixel);
                    opaque.push(pixel);
                }
            }
        }
        if opaque.is_empty() {
            continue;
        }
        let center = (min.as_vec2() + max.as_vec2()) * 0.5;
        let pixel = opaque
            .into_iter()
            .min_by(|a, b| {
                a.as_vec2()
                    .distance_squared(center)
                    .total_cmp(&b.as_vec2().distance_squared(center))
            })
            .unwrap();
        let mut normalized =
            (pixel.as_vec2() + Vec2::splat(0.5) - rect.min) / rect.size() - Vec2::splat(0.5);
        if sprite.flip_x {
            normalized.x *= -1.0;
        }
        if !sprite.flip_y {
            normalized.y *= -1.0;
        }
        let anchor = world
            .get::<bevy::sprite::Anchor>(id)
            .copied()
            .unwrap_or_default();
        let local = (normalized - anchor.as_vec()) * sprite.custom_size.unwrap_or(rect.size());
        candidates.push((
            // A control's decorative descendants (for example hover brackets)
            // must not displace its artwork sample. Explicitly selecting that
            // decoration still observes it, and decorative-only groups retain
            // their normal geometry fallback. This does not promise clickability.
            id == root || world.get::<Pickable>(id).is_none_or(|p| p.is_hoverable),
            transform.translation().z,
            transform.transform_point(local.extend(0.0)),
        ));
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
    candidates
        .last()
        .map(|(_, _, point)| *point)
        .ok_or_else(|| "entity has no visible sprite pixels; select a visible control".into())
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use inslag_state_tree::StateTreePlugin;

    #[test]
    fn screen_coordinates_remove_dpi_but_not_ui_scale() {
        let lua = Lua::new();
        for dpi in [1.0, 1.25, 2.0] {
            let mut world = World::new();
            world.insert_resource(UiScale(2.0 / dpi));
            let entity = world
                .spawn((
                    Node::default(),
                    ComputedNode {
                        size: Vec2::new(120., 60.),
                        inverse_scale_factor: 0.5,
                        ..default()
                    },
                    UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::new(
                        400., 200.,
                    ))),
                ))
                .id();
            let args = lua.create_table().unwrap();
            args.set("entity", entity.to_bits().to_string()).unwrap();
            let point = screen(&world, &lua, &args).unwrap();
            let point = point.as_table().unwrap();
            assert_eq!(point.get::<f32>("x").unwrap(), 400. / dpi);
            assert_eq!(point.get::<f32>("y").unwrap(), 200. / dpi);
        }
    }

    #[test]
    fn trees_are_constructed_observed_and_checked_without_domain_registration() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("chart.scxml"), r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" datamodel="luau"><datamodel><data id="count" expr="0"/></datamodel><state id="active"><onentry><assign location="count" expr="count + config.extra"/></onentry></state></scxml>"#).unwrap();
        let mut app = App::new();
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::time::TimePlugin,
            bevy::asset::AssetPlugin {
                file_path: directory.path().to_str().unwrap().into(),
                ..default()
            },
            StateTreePlugin::new(Update),
        ));
        let owner = app.world_mut().spawn_empty().id();
        let lua = Lua::new();
        let args = lua.create_table().unwrap();
        args.set("entity", owner.to_bits().to_string()).unwrap();
        args.set("asset", "chart.scxml").unwrap();
        args.set("data", lua.create_table_from([("count", 7)]).unwrap())
            .unwrap();
        args.set("config", lua.create_table_from([("extra", 2)]).unwrap())
            .unwrap();
        tree(app.world_mut(), &lua, &args, true).unwrap();
        assert!(!trees_ready(app.world_mut()).unwrap());
        for _ in 0..1000 {
            app.update();
            if trees_ready(app.world_mut()).unwrap() {
                break;
            }
            std::thread::yield_now();
        }
        assert!(trees_ready(app.world_mut()).unwrap());
        args.set("key", "count").unwrap();
        assert_eq!(
            tree(app.world_mut(), &lua, &args, false)
                .unwrap()
                .as_integer(),
            Some(9)
        );
        let snapshot = tree_snapshot(app.world_mut());
        assert_eq!(snapshot.as_array().unwrap().len(), 1);
        assert_eq!(
            snapshot[0]["states"].as_array().unwrap().last().unwrap(),
            "active"
        );
    }
}
