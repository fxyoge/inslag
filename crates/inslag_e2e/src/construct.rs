//! Reflected setup construction. Fixture knowledge belongs in consumer Luau.
use bevy::reflect::{map::Map, set::Set};
use bevy::{
    prelude::Entity,
    reflect::{
        PartialReflect, ReflectFromReflect, TypeInfo, TypeRegistration, TypeRegistry,
        array::DynamicArray,
        enums::{DynamicEnum, DynamicVariant, VariantInfo},
        list::DynamicList,
        map::DynamicMap,
        set::DynamicSet,
        structs::DynamicStruct,
        tuple::DynamicTuple,
        tuple_struct::DynamicTupleStruct,
    },
};
use mlua::{Lua, Table, Value};
use serde::de::DeserializeSeed;
use std::any::TypeId;

pub(crate) fn decode(
    lua: &Lua,
    value: Value,
    registration: &TypeRegistration,
    registry: &TypeRegistry,
    depth: usize,
) -> Result<Box<dyn PartialReflect>, String> {
    if depth > 32 {
        return Err("reflected input exceeds nesting limit".into());
    }
    if registration.type_id() == TypeId::of::<Entity>() {
        let id = value
            .as_string()
            .ok_or("Entity fields require string IDs")?
            .to_str()
            .map_err(err)?;
        return Ok(Box::new(super::ecs::entity(&id)?));
    }
    let info = registration.type_info();
    let child = |value, id| {
        let registration = registry
            .get(id)
            .ok_or_else(|| format!("unregistered nested reflected type {id:?}"))?;
        decode(lua, value, registration, registry, depth + 1)
    };
    let table = || {
        value
            .as_table()
            .ok_or_else(|| format!("{} requires a table", info.type_path()))
    };
    let output: Box<dyn PartialReflect> = match info {
        TypeInfo::Struct(info) => {
            let input = table()?;
            let mut output = DynamicStruct::default();
            output.set_represented_type(Some(registration.type_info()));
            for (i, field) in info.iter().enumerate() {
                let value = field_value(input, field.name(), i)?;
                output.insert_boxed(field.name(), child(value, field.type_id())?);
            }
            for pair in input.clone().pairs::<Value, Value>() {
                let (key, _) = pair.map_err(err)?;
                if let Value::String(key) = key
                    && info.field(&key.to_str().map_err(err)?).is_none()
                {
                    return Err(format!(
                        "unknown field {} on {}",
                        key.to_string_lossy(),
                        info.type_path()
                    ));
                }
            }
            Box::new(output)
        }
        TypeInfo::TupleStruct(info) => {
            let input = table()?;
            let mut output = DynamicTupleStruct::default();
            output.set_represented_type(Some(registration.type_info()));
            if input.raw_len() != info.field_len() {
                return Err("wrong tuple struct length".into());
            }
            for (i, field) in info.iter().enumerate() {
                output.insert_boxed(child(input.raw_get(i + 1).map_err(err)?, field.type_id())?);
            }
            Box::new(output)
        }
        TypeInfo::Tuple(info) => {
            let input = table()?;
            let mut output = DynamicTuple::default();
            output.set_represented_type(Some(registration.type_info()));
            if input.raw_len() != info.field_len() {
                return Err("wrong tuple length".into());
            }
            for (i, field) in info.iter().enumerate() {
                output.insert_boxed(child(input.raw_get(i + 1).map_err(err)?, field.type_id())?);
            }
            Box::new(output)
        }
        TypeInfo::List(info) => {
            let input = table()?;
            check_len(input)?;
            let mut output = DynamicList::default();
            output.set_represented_type(Some(registration.type_info()));
            for value in input.clone().sequence_values::<Value>() {
                output.push_box(child(value.map_err(err)?, info.item_ty().id())?);
            }
            Box::new(output)
        }
        TypeInfo::Array(info) => {
            let input = table()?;
            check_len(input)?;
            if input.raw_len() != info.capacity() {
                return Err("wrong reflected array length".into());
            }
            let values = input
                .clone()
                .sequence_values::<Value>()
                .map(|v| child(v.map_err(err)?, info.item_ty().id()))
                .collect::<Result<Vec<_>, _>>()?;
            let mut output = DynamicArray::new(values.into_boxed_slice());
            output.set_represented_type(Some(registration.type_info()));
            Box::new(output)
        }
        TypeInfo::Map(info) => {
            let input = table()?;
            check_len(input)?;
            let mut output = DynamicMap::default();
            output.set_represented_type(Some(registration.type_info()));
            for entry in input.clone().sequence_values::<Table>() {
                let entry = entry.map_err(err)?;
                let key = child(entry.get("key").map_err(err)?, info.key_ty().id())?;
                let value = child(entry.get("value").map_err(err)?, info.value_ty().id())?;
                if key.reflect_hash().is_none() {
                    return Err("reflected map key does not support hashing".into());
                }
                output.insert_boxed(key, value);
            }
            Box::new(output)
        }
        TypeInfo::Set(info) => {
            let input = table()?;
            check_len(input)?;
            let mut output = DynamicSet::default();
            output.set_represented_type(Some(registration.type_info()));
            for value in input.clone().sequence_values::<Value>() {
                let value = child(value.map_err(err)?, info.value_ty().id())?;
                if value.reflect_hash().is_none() {
                    return Err("reflected set value does not support hashing".into());
                }
                output.insert_boxed(value);
            }
            Box::new(output)
        }
        TypeInfo::Enum(info) => {
            let (name, fields) = if let Value::String(name) = &value {
                (
                    name.to_str().map_err(err)?.to_string(),
                    lua.create_table().map_err(err)?,
                )
            } else {
                let input = table()?;
                (
                    input.get::<String>("variant").map_err(err)?,
                    input.get::<Table>("fields").map_err(err)?,
                )
            };
            let variant = info
                .variant(&name)
                .ok_or_else(|| format!("unknown variant {name} on {}", info.type_path()))?;
            let variant = match variant {
                VariantInfo::Unit(_) => DynamicVariant::Unit,
                VariantInfo::Struct(info) => {
                    let mut output = DynamicStruct::default();
                    for (i, field) in info.iter().enumerate() {
                        output.insert_boxed(
                            field.name(),
                            child(field_value(&fields, field.name(), i)?, field.type_id())?,
                        );
                    }
                    DynamicVariant::Struct(output)
                }
                VariantInfo::Tuple(info) => {
                    if fields.raw_len() != info.field_len() {
                        return Err("wrong variant tuple length".into());
                    }
                    let mut output = DynamicTuple::default();
                    for (i, field) in info.iter().enumerate() {
                        output.insert_boxed(child(
                            fields.raw_get(i + 1).map_err(err)?,
                            field.type_id(),
                        )?);
                    }
                    DynamicVariant::Tuple(output)
                }
            };
            let mut output =
                DynamicEnum::new_with_index(info.index_of(&name).unwrap(), name, variant);
            output.set_represented_type(Some(registration.type_info()));
            Box::new(output)
        }
        TypeInfo::Opaque(_) => {
            return bevy::reflect::serde::TypedReflectDeserializer::new(registration, registry)
                .deserialize(mlua::serde::Deserializer::new(value))
                .map_err(err);
        }
    };
    // Materialize keys/values so map hashing and FromReflect work exactly as for
    // normal Rust values, rather than relying on DynamicStruct's hash support.
    if let Some(converter) = registration.data::<ReflectFromReflect>() {
        return converter
            .from_reflect(output.as_ref())
            .map(PartialReflect::into_partial_reflect)
            .ok_or_else(|| {
                format!(
                    "cannot construct {} from reflected fields",
                    info.type_path()
                )
            });
    }
    Ok(output)
}

fn field_value(input: &Table, name: &str, index: usize) -> Result<Value, String> {
    let value: Value = input.get(name).map_err(err)?;
    if !value.is_nil() {
        return Ok(value);
    }
    let value: Value = input.raw_get(index + 1).map_err(err)?;
    if value.is_nil() {
        return Err(format!("missing reflected field {name}"));
    }
    Ok(value)
}

fn check_len(input: &Table) -> Result<(), String> {
    if input.raw_len() > 10000 {
        return Err("reflected collection exceeds 10000 entries".into());
    }
    let entries = input.clone().pairs::<Value, Value>().count();
    if entries != input.raw_len() {
        return Err(
            "reflected collections require a dense sequence; maps use {key=..., value=...} entries"
                .into(),
        );
    }
    Ok(())
}
fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
