use crate::{Arguments, FromLootValue, FunctionSignature, LootError, Value, ValueType};
use std::{
    fmt::{Debug, Display},
    sync::Arc,
};

/// Named arguments exposed to loot sources.
pub trait LootArgs: Sized {
    #[doc(hidden)]
    fn signature() -> FunctionSignature;
    #[doc(hidden)]
    fn from_arguments(args: &mut Arguments) -> Result<Self, String>;
}

impl LootArgs for () {
    fn signature() -> FunctionSignature {
        FunctionSignature::new()
    }
    fn from_arguments(_: &mut Arguments) -> Result<Self, String> {
        Ok(())
    }
}

/// A value usable in loot expressions.
pub trait LootValue {
    #[doc(hidden)]
    fn value_type() -> ValueType;
    #[doc(hidden)]
    fn into_value(self) -> Value;
}

/// A generator's contribution to the result bag.
pub trait LootReturn {
    /// `None` indicates a generator with no output.
    fn output_type() -> Option<ValueType>;
    /// Flattens the return value into bag entries.
    fn into_values(self) -> Result<Vec<Value>, String>;
}

/// A consumer-facing loot result.
pub trait LootOutput: Sized + Send + Sync + 'static {
    #[doc(hidden)]
    fn accepts_type(value_type: &ValueType) -> bool;
    #[doc(hidden)]
    fn accepts_value(value: &Value) -> bool;
    #[doc(hidden)]
    fn from_value(value: Value) -> Result<Self, String>;
}

impl LootReturn for () {
    fn output_type() -> Option<ValueType> {
        None
    }
    fn into_values(self) -> Result<Vec<Value>, String> {
        Ok(Vec::new())
    }
}

impl<T: LootValue> LootReturn for Vec<T> {
    fn output_type() -> Option<ValueType> {
        Some(T::value_type())
    }
    fn into_values(self) -> Result<Vec<Value>, String> {
        Ok(self.into_iter().map(T::into_value).collect())
    }
}

impl<T: LootValue> LootReturn for Option<T> {
    fn output_type() -> Option<ValueType> {
        Some(T::value_type())
    }
    fn into_values(self) -> Result<Vec<Value>, String> {
        Ok(self.into_iter().map(T::into_value).collect())
    }
}

impl<T: LootReturn, E: Display> LootReturn for Result<T, E> {
    fn output_type() -> Option<ValueType> {
        T::output_type()
    }
    fn into_values(self) -> Result<Vec<Value>, String> {
        self.map_err(|error| error.to_string())?.into_values()
    }
}

macro_rules! atom {
    ($ty:ty, $value_type:expr, $convert:expr) => {
        impl LootValue for $ty {
            fn value_type() -> ValueType {
                $value_type
            }
            fn into_value(self) -> Value {
                ($convert)(self)
            }
        }
        impl LootReturn for $ty {
            fn output_type() -> Option<ValueType> {
                Some(<Self as LootValue>::value_type())
            }
            fn into_values(self) -> Result<Vec<Value>, String> {
                Ok(vec![self.into_value()])
            }
        }
        impl LootOutput for $ty {
            fn accepts_type(value_type: &ValueType) -> bool {
                <Self as LootValue>::value_type().contains(value_type)
            }
            fn accepts_value(value: &Value) -> bool {
                <Self as LootValue>::value_type().accepts(value)
            }
            fn from_value(value: Value) -> Result<Self, String> {
                Self::from_loot_value(value)
            }
        }
    };
}

atom!(Value, ValueType::Any, |value| value);
atom!(String, ValueType::String, Value::from);
atom!(bool, ValueType::Boolean, Value::from);
atom!(i64, ValueType::Integer, Value::from);
atom!(
    u32,
    ValueType::IntegerRange {
        min: 0,
        max: u32::MAX as i64
    },
    |value: u32| Value::Integer(value.into())
);
atom!(f64, ValueType::Number, Value::from);
atom!(crate::Reference, ValueType::AnyReference, Value::from);
atom!(
    inslag_luau::LuauValue,
    ValueType::OneOf(Arc::from([
        ValueType::Nil,
        ValueType::Boolean,
        ValueType::Number,
        ValueType::String,
        ValueType::list(ValueType::Any),
        ValueType::Record(Box::new(ValueType::Any)),
    ])),
    Value::from
);

impl LootValue for &str {
    fn value_type() -> ValueType {
        ValueType::String
    }

    fn into_value(self) -> Value {
        Value::from(self)
    }
}

impl<T: LootValue> LootValue for Vec<T> {
    fn value_type() -> ValueType {
        ValueType::list(T::value_type())
    }
    fn into_value(self) -> Value {
        Value::List(self.into_iter().map(T::into_value).collect())
    }
}

impl<T: Send + Sync + 'static> FromLootValue for Arc<T> {
    fn from_loot_value(value: Value) -> Result<Self, String> {
        match value {
            Value::Data(data) => data
                .downcast()
                .map_err(|_| format!("expected {}", std::any::type_name::<T>())),
            _ => Err("expected generated data".into()),
        }
    }
}

impl<T: Debug + PartialEq + Send + Sync + 'static> LootValue for Arc<T> {
    fn value_type() -> ValueType {
        ValueType::data::<T>()
    }
    fn into_value(self) -> Value {
        Value::Data(crate::Data::from_arc(self))
    }
}

pub(crate) fn convert_outputs<T: LootOutput>(values: Vec<Value>) -> Result<Vec<T>, LootError> {
    values
        .into_iter()
        .map(|value| T::from_value(value).map_err(LootError::request))
        .collect()
}
