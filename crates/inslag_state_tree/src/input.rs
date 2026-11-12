use std::collections::BTreeMap;

use bevy_ecs::entity::Entity;
use bevy_math::{Vec2, Vec3, Vec4};

use crate::{
    FromStateTreeValue, LuauValue, StateTreeActivityArgs, StateTreeActivityContext,
    StateTreeActivityError,
};

pub trait StateTreeInput: Sized {
    fn from_state_tree_input(
        context: &StateTreeActivityContext<'_>,
        arguments: &mut StateTreeActivityArgs,
    ) -> Result<Self, StateTreeActivityError>;
}

impl<T: FromStateTreeValue> FromStateTreeValue for Option<T> {
    fn from_state_tree_value(value: LuauValue) -> Result<Self, Box<str>> {
        match value {
            LuauValue::Nil => Ok(None),
            value => T::from_state_tree_value(value).map(Some),
        }
    }

    fn missing_value() -> Option<Self> {
        Some(None)
    }
}

impl<T: FromStateTreeValue> FromStateTreeValue for Vec<T> {
    fn from_state_tree_value(value: LuauValue) -> Result<Self, Box<str>> {
        let LuauValue::Sequence(values) = value else {
            return Err("expected a sequence".into());
        };
        values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                T::from_state_tree_value(value)
                    .map_err(|error| format!("[{index}]: {error}").into())
            })
            .collect()
    }
}

impl<T: FromStateTreeValue, const N: usize> FromStateTreeValue for [T; N] {
    fn from_state_tree_value(value: LuauValue) -> Result<Self, Box<str>> {
        Vec::<T>::from_state_tree_value(value)?
            .try_into()
            .map_err(|values: Vec<T>| format!("expected {N} elements, got {}", values.len()).into())
    }
}

impl<T: FromStateTreeValue> FromStateTreeValue for BTreeMap<String, T> {
    fn from_state_tree_value(value: LuauValue) -> Result<Self, Box<str>> {
        let values = match value {
            LuauValue::Record(values) => values,
            LuauValue::Sequence(values) if values.is_empty() => return Ok(Self::new()),
            _ => return Err("expected a record".into()),
        };
        values
            .into_iter()
            .map(|(key, value)| {
                let value = T::from_state_tree_value(value)
                    .map_err(|error| Box::<str>::from(format!("{key}: {error}")))?;
                Ok((key, value))
            })
            .collect()
    }
}

impl FromStateTreeValue for Entity {
    fn from_state_tree_value(value: LuauValue) -> Result<Self, Box<str>> {
        let bits = String::from_state_tree_value(value)?
            .parse::<u64>()
            .map_err(|_| Box::<str>::from("expected an entity identity"))?;
        Entity::try_from_bits(bits).ok_or_else(|| "expected an entity identity".into())
    }
}

impl FromStateTreeValue for bevy_asset::AssetPath<'static> {
    fn from_state_tree_value(value: LuauValue) -> Result<Self, Box<str>> {
        bevy_asset::AssetPath::try_parse(&String::from_state_tree_value(value)?)
            .map(|path| path.into_owned())
            .map_err(|error| error.to_string().into())
    }
}

macro_rules! integers {
    ($($ty:ty),* $(,)?) => {$(
        impl FromStateTreeValue for $ty {
            fn from_state_tree_value(value: LuauValue) -> Result<Self, Box<str>> {
                let value = i64::from_state_tree_value(value)?;
                Self::try_from(value).map_err(|_| concat!("integer is outside the ", stringify!($ty), " range").into())
            }
        }
    )*};
}

integers!(i8, i16, i32, isize, u8, u16, u64, usize);

macro_rules! vectors {
    ($($ty:ty => [$($axis:ident),*]),* $(,)?) => {$(
        impl FromStateTreeValue for $ty {
            fn from_state_tree_value(value: LuauValue) -> Result<Self, Box<str>> {
                let value = if let LuauValue::Record(mut fields) = value {
                    let value = Self::new($(
                        f32::from_state_tree_value(fields.remove(stringify!($axis))
                            .ok_or_else(|| Box::<str>::from(concat!("missing coordinate ", stringify!($axis))))?)?
                    ),*);
                    if !fields.is_empty() {
                        return Err("unexpected coordinate field".into());
                    }
                    value
                } else {
                    Self::from_array(FromStateTreeValue::from_state_tree_value(value)?)
                };
                Ok(value)
            }
        }
    )*};
}

vectors!(Vec2 => [x, y], Vec3 => [x, y, z], Vec4 => [x, y, z, w]);

impl FromStateTreeValue for bevy_math::Rect {
    fn from_state_tree_value(value: LuauValue) -> Result<Self, Box<str>> {
        let [left, bottom, right, top] = <[f32; 4]>::from_state_tree_value(value)?;
        Ok(Self::new(left, bottom, right, top))
    }
}

impl FromStateTreeValue for bevy_math::UVec2 {
    fn from_state_tree_value(value: LuauValue) -> Result<Self, Box<str>> {
        <[u32; 2]>::from_state_tree_value(value).map(Self::from_array)
    }
}
