use crate::{LootError, Reference, SourceLocation, Value, ValueType};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    pub value_type: ValueType,
    pub default: Option<Value>,
    pub optional: bool,
}

/// Argument and output declarations used to validate calls in loot sources.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FunctionSignature {
    pub(crate) parameters: BTreeMap<String, Parameter>,
    pub(crate) primary: Option<String>,
    pub(crate) output: Option<ValueType>,
}

impl FunctionSignature {
    /// Creates a signature with no arguments or output.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares an argument that callers must supply.
    ///
    /// # Panics
    ///
    /// Panics if an argument with this name is already declared.
    pub fn required(mut self, name: &str, value_type: ValueType) -> Self {
        assert!(
            !self.parameters.contains_key(name),
            "duplicate parameter {name}"
        );
        self.parameters.insert(
            name.into(),
            Parameter {
                value_type,
                default: None,
                optional: false,
            },
        );
        self
    }

    /// Declares an argument whose default is supplied when omitted.
    ///
    /// # Panics
    ///
    /// Panics if an argument with this name is already declared.
    pub fn optional(
        mut self,
        name: &str,
        value_type: ValueType,
        default: impl Into<Value>,
    ) -> Self {
        assert!(
            !self.parameters.contains_key(name),
            "duplicate parameter {name}"
        );
        self.parameters.insert(
            name.into(),
            Parameter {
                value_type,
                default: Some(default.into()),
                optional: true,
            },
        );
        self
    }

    /// Selects the argument that accepts a call's unnamed value.
    pub fn primary(mut self, name: &str) -> Self {
        self.primary = Some(name.into());
        self
    }
    /// Allows an argument to be absent without supplying a default.
    ///
    /// # Panics
    ///
    /// Panics if an argument with this name is already declared.
    pub fn omittable(mut self, name: &str, value_type: ValueType) -> Self {
        assert!(
            !self.parameters.contains_key(name),
            "duplicate parameter {name}"
        );
        self.parameters.insert(
            name.into(),
            Parameter {
                value_type,
                default: None,
                optional: true,
            },
        );
        self
    }
    /// Declares the type of each value added to the result bag.
    pub fn emits(mut self, output: ValueType) -> Self {
        self.output = Some(output);
        self
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        for (name, parameter) in &self.parameters {
            if !inslag_format_loot::is_name(name) || inslag_format_loot::is_reserved(name) {
                return Err(format!("invalid or reserved parameter {name:?}"));
            }
            if parameter
                .default
                .as_ref()
                .is_some_and(|v| !parameter.value_type.accepts(v))
            {
                return Err(format!("invalid default for parameter {name:?}"));
            }
        }
        if self
            .primary
            .as_ref()
            .is_some_and(|name| !self.parameters.contains_key(name))
        {
            return Err("primary parameter is not declared".into());
        }
        Ok(())
    }

    pub(crate) fn arguments(
        &self,
        mut values: BTreeMap<String, Value>,
        location: &SourceLocation,
    ) -> Result<Arguments, LootError> {
        for name in values.keys() {
            if !self.parameters.contains_key(name) {
                return Err(LootError::new(
                    location.clone(),
                    format!("unknown argument {name:?}"),
                ));
            }
        }
        for (name, parameter) in &self.parameters {
            if !values.contains_key(name) {
                let Some(default) = &parameter.default else {
                    if parameter.optional {
                        continue;
                    }
                    return Err(LootError::new(
                        location.clone(),
                        format!("missing argument {name:?}"),
                    ));
                };
                values.insert(name.clone(), default.clone());
            }
            if !parameter.value_type.accepts(&values[name]) {
                return Err(LootError::new(
                    location.clone(),
                    format!(
                        "argument {name:?} expects {:?}, got {:?}",
                        parameter.value_type, values[name]
                    ),
                ));
            }
        }
        Ok(Arguments(values))
    }

    /// Iterates argument declarations in name order.
    pub fn parameters(&self) -> impl Iterator<Item = (&str, &Parameter)> {
        self.parameters
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }
    /// Returns the argument selected to accept unnamed values.
    pub fn primary_parameter(&self) -> Option<&str> {
        self.primary.as_deref()
    }
    /// Returns the emitted type, or `None` if the generator emits nothing.
    pub fn output(&self) -> Option<&ValueType> {
        self.output.as_ref()
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FunctionSignatures(pub(crate) BTreeMap<String, FunctionSignature>);

impl FunctionSignatures {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::default()
    }
    pub(crate) fn register(
        &mut self,
        name: &str,
        signature: FunctionSignature,
    ) -> Result<&mut Self, String> {
        if !inslag_format_loot::is_name(name) {
            return Err(format!("invalid function name {name:?}"));
        }
        signature.validate()?;
        if self.0.contains_key(name) {
            return Err(format!("function {name:?} already registered"));
        }
        self.0.insert(name.into(), signature);
        Ok(self)
    }
    pub(crate) fn get(&self, name: &str) -> Option<&FunctionSignature> {
        self.0.get(name)
    }
}

/// Validated call arguments. Every supplied argument must be consumed.
#[derive(Clone, Debug, Default)]
pub struct Arguments(pub(crate) BTreeMap<String, Value>);

impl Arguments {
    /// Removes and converts an argument, returning `None` if absent.
    /// Conversion errors still consume the argument.
    pub fn take_optional<T: FromLootValue>(&mut self, name: &str) -> Result<Option<T>, String> {
        self.0
            .remove(name)
            .map(T::from_loot_value)
            .transpose()
            .map_err(|error| format!("argument {name:?}: {error}"))
    }
    /// Removes generated Rust data, failing if absent or not of type `T`.
    pub fn take_data<T: Send + Sync + 'static>(
        &mut self,
        name: &str,
    ) -> Result<std::sync::Arc<T>, String> {
        match self.take::<Value>(name)? {
            Value::Data(data) => data
                .downcast()
                .map_err(|_| format!("argument {name:?}: expected {}", std::any::type_name::<T>())),
            _ => Err(format!("argument {name:?}: expected generated data")),
        }
    }
    /// Removes and converts a required argument, failing if absent or incompatible.
    /// Conversion errors still consume the argument.
    pub fn take<T: FromLootValue>(&mut self, name: &str) -> Result<T, String> {
        let value = self
            .0
            .remove(name)
            .ok_or_else(|| format!("missing argument {name:?}"))?;
        T::from_loot_value(value).map_err(|error| format!("argument {name:?}: {error}"))
    }
    /// Borrows an argument without marking it consumed.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name)
    }
    /// Borrows remaining arguments in name order without consuming them.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }
    pub(crate) fn finish(&self) -> Result<(), String> {
        if let Some(name) = self.0.keys().next() {
            Err(format!("argument {name:?} was not consumed"))
        } else {
            Ok(())
        }
    }
}

/// Conversion from a loot value into a generator argument.
pub trait FromLootValue: Sized {
    /// Rejects values that cannot be represented by this type.
    fn from_loot_value(value: Value) -> Result<Self, String>;
}

impl FromLootValue for Value {
    fn from_loot_value(value: Value) -> Result<Self, String> {
        Ok(value)
    }
}
impl FromLootValue for inslag_luau::LuauValue {
    fn from_loot_value(value: Value) -> Result<Self, String> {
        value.to_luau()
    }
}
impl FromLootValue for String {
    fn from_loot_value(value: Value) -> Result<Self, String> {
        if let Value::String(value) = value {
            Ok(value.to_string())
        } else {
            Err("expected string".into())
        }
    }
}
impl FromLootValue for Reference {
    fn from_loot_value(value: Value) -> Result<Self, String> {
        if let Value::Reference(value) = value {
            Ok(value)
        } else {
            Err("expected reference".into())
        }
    }
}
impl FromLootValue for i64 {
    fn from_loot_value(value: Value) -> Result<Self, String> {
        if let Value::Integer(value) = value {
            Ok(value)
        } else {
            Err("expected integer".into())
        }
    }
}
impl FromLootValue for u32 {
    fn from_loot_value(value: Value) -> Result<Self, String> {
        i64::from_loot_value(value)?
            .try_into()
            .map_err(|_| "expected unsigned 32-bit integer".into())
    }
}
impl FromLootValue for bool {
    fn from_loot_value(value: Value) -> Result<Self, String> {
        if let Value::Boolean(value) = value {
            Ok(value)
        } else {
            Err("expected boolean".into())
        }
    }
}
impl FromLootValue for f64 {
    fn from_loot_value(value: Value) -> Result<Self, String> {
        match value {
            Value::Number(value) => Ok(value),
            Value::Integer(value) => Ok(value as f64),
            _ => Err("expected number".into()),
        }
    }
}
impl<T: FromLootValue> FromLootValue for Vec<T> {
    fn from_loot_value(value: Value) -> Result<Self, String> {
        if let Value::List(values) = value {
            values.iter().cloned().map(T::from_loot_value).collect()
        } else {
            Err("expected list".into())
        }
    }
}
