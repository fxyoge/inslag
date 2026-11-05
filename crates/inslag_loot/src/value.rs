use inslag_luau::LuauValue;
use std::{
    any::{Any, TypeId, type_name},
    collections::BTreeMap,
    fmt::{self, Debug},
    sync::Arc,
};

#[derive(Clone)]
pub struct Data {
    value: Arc<dyn Any + Send + Sync>,
    format: fn(&dyn Any, &mut fmt::Formatter<'_>) -> fmt::Result,
    equal: fn(&dyn Any, &dyn Any) -> bool,
}

impl Data {
    pub fn new<T: Debug + PartialEq + Send + Sync + 'static>(value: T) -> Self {
        Self::from_arc(Arc::new(value))
    }

    pub fn from_arc<T: Debug + PartialEq + Send + Sync + 'static>(value: Arc<T>) -> Self {
        Self {
            value,
            format: |value, f| {
                value
                    .downcast_ref::<T>()
                    .expect("matching data type")
                    .fmt(f)
            },
            equal: |left, right| left.downcast_ref::<T>() == right.downcast_ref::<T>(),
        }
    }

    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.value.downcast_ref()
    }
    pub fn downcast<T: Send + Sync + 'static>(self) -> Result<Arc<T>, Self> {
        match self.value.clone().downcast::<T>() {
            Ok(value) => Ok(value),
            Err(_) => Err(self),
        }
    }
    pub fn type_id(&self) -> TypeId {
        self.value.as_ref().type_id()
    }
}

impl Debug for Data {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (self.format)(self.value.as_ref(), f)
    }
}

impl PartialEq for Data {
    fn eq(&self, other: &Self) -> bool {
        self.type_id() == other.type_id() && (self.equal)(self.value.as_ref(), other.value.as_ref())
    }
}

/// An application-defined reference, compared by both kind and ID.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Reference {
    /// Distinguishes references to different kinds of application data.
    pub kind: Arc<str>,
    /// Identifies a value within its kind; loot generation does not validate it.
    pub id: u64,
}

impl Reference {
    /// Creates a reference without checking whether its target exists.
    pub fn new(kind: impl Into<Arc<str>>, id: u64) -> Self {
        Self {
            kind: kind.into(),
            id,
        }
    }
}

/// A value exchanged between loot expressions and native generators.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// An absent Luau value.
    Nil,
    /// A truth value.
    Boolean(bool),
    /// A signed whole number.
    Integer(i64),
    /// A floating-point number; non-finite values are rejected during validation.
    Number(f64),
    /// Shared text.
    String(Arc<str>),
    /// An application-defined reference, not automatically resolved.
    Reference(Reference),
    /// An ordered collection of loot values.
    List(Arc<[Value]>),
    /// String-keyed fields accessible to Luau guards.
    Record(Arc<BTreeMap<String, Value>>),
    /// Opaque Rust data produced by a native generator.
    Data(Data),
}

/// Constraints checked when compiling sources and validating generated values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueType {
    /// Any valid loot value.
    Any,
    /// An absent Luau value.
    Nil,
    /// Any of the listed types.
    OneOf(Arc<[ValueType]>),
    /// Boolean values only.
    Boolean,
    /// Integer values only.
    Integer,
    /// Floating-point or integer values.
    Number,
    /// Any string.
    String,
    /// Only the listed strings, matched exactly.
    StringEnum(&'static [&'static str]),
    /// Integers within inclusive bounds.
    IntegerRange {
        /// Smallest accepted integer.
        min: i64,
        /// Largest accepted integer.
        max: i64,
    },
    /// References of any kind.
    AnyReference,
    /// References whose kind matches exactly.
    Reference(Arc<str>),
    /// Lists whose elements all satisfy the given type.
    List(Box<ValueType>),
    /// Records whose fields all satisfy the given type.
    Record(Box<ValueType>),
    /// Opaque Rust data matched by its concrete type.
    Data {
        /// Concrete Rust type used for matching.
        id: TypeId,
        /// Rust type name used in diagnostics.
        name: &'static str,
    },
}

impl Value {
    /// Wraps opaque Rust data for use in loot expressions.
    pub fn data<T: Debug + PartialEq + Send + Sync + 'static>(value: T) -> Self {
        Self::Data(Data::new(value))
    }
    /// Borrows opaque data if its concrete type is `T`; otherwise returns `None`.
    pub fn get_data<T: 'static>(&self) -> Option<&T> {
        if let Self::Data(data) = self {
            data.get()
        } else {
            None
        }
    }
    /// Rejects non-finite numbers, including those nested in lists.
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Number(value) => value.is_finite(),
            Self::List(values) => values.iter().all(Self::is_valid),
            Self::Record(values) => values.values().all(Self::is_valid),
            _ => true,
        }
    }

    pub(crate) fn to_luau(&self) -> Result<LuauValue, String> {
        Ok(match self {
            Self::Nil => LuauValue::Nil,
            Self::Boolean(value) => LuauValue::Boolean(*value),
            Self::Integer(value) => LuauValue::Integer(*value),
            Self::Number(value) => LuauValue::Number(*value),
            Self::String(value) => LuauValue::String(value.to_string()),
            Self::List(values) => {
                LuauValue::Sequence(values.iter().map(Self::to_luau).collect::<Result<_, _>>()?)
            }
            Self::Record(values) => LuauValue::Record(
                values
                    .iter()
                    .map(|(key, value)| Ok((key.clone(), value.to_luau()?)))
                    .collect::<Result<_, String>>()?,
            ),
            Self::Reference(_) => {
                return Err(
                    "references are opaque in Luau expressions; pass the required values as inputs"
                        .into(),
                );
            }
            Self::Data(_) => {
                return Err(
                    "Rust data is opaque in Luau expressions; pass the required values as inputs"
                        .into(),
                );
            }
        })
    }
}

impl ValueType {
    /// Tests whether every value allowed by `other` is also allowed by this type.
    pub fn contains(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Any, _) => true,
            (_, Self::OneOf(types)) => types.iter().all(|ty| self.contains(ty)),
            (Self::OneOf(types), _) => types.iter().any(|ty| ty.contains(other)),
            (Self::Data { id: left, .. }, Self::Data { id: right, .. }) => left == right,
            (Self::Integer, Self::IntegerRange { .. }) => true,
            (Self::Number, Self::Integer | Self::IntegerRange { .. }) => true,
            (Self::String, Self::StringEnum(_)) => true,
            (Self::StringEnum(left), Self::StringEnum(right)) => {
                right.iter().all(|v| left.contains(v))
            }
            (Self::AnyReference, Self::Reference(_)) => true,
            (
                Self::IntegerRange {
                    min: left_min,
                    max: left_max,
                },
                Self::IntegerRange { min, max },
            ) => left_min <= min && max <= left_max,
            (Self::List(left), Self::List(right)) => left.contains(right),
            (Self::Record(left), Self::Record(right)) => left.contains(right),
            _ => self == other,
        }
    }
    /// Requires opaque data with exactly the Rust type `T`.
    pub fn data<T: 'static>() -> Self {
        Self::Data {
            id: TypeId::of::<T>(),
            name: type_name::<T>(),
        }
    }
    /// Requires references with the given kind.
    pub fn reference(kind: impl Into<Arc<str>>) -> Self {
        Self::Reference(kind.into())
    }
    /// Requires every list element to satisfy `element`.
    pub fn list(element: Self) -> Self {
        Self::List(Box::new(element))
    }

    /// Checks a value against this constraint. Invalid numbers are always rejected.
    pub fn accepts(&self, value: &Value) -> bool {
        if !value.is_valid() {
            return false;
        }
        match (self, value) {
            (Self::Any, _) => true,
            (Self::OneOf(types), _) => types.iter().any(|ty| ty.accepts(value)),
            (Self::Nil, Value::Nil)
            | (Self::Boolean, Value::Boolean(_))
            | (Self::Integer, Value::Integer(_))
            | (Self::Number, Value::Integer(_) | Value::Number(_))
            | (Self::String, Value::String(_)) => true,
            (Self::StringEnum(variants), Value::String(value)) => {
                variants.contains(&value.as_ref())
            }
            (Self::IntegerRange { min, max }, Value::Integer(value)) => {
                (min..=max).contains(&value)
            }
            (Self::AnyReference, Value::Reference(_)) => true,
            (Self::Reference(kind), Value::Reference(reference)) => *kind == reference.kind,
            (Self::List(element), Value::List(values)) => values.iter().all(|v| element.accepts(v)),
            (Self::Record(element), Value::Record(values)) => {
                values.values().all(|v| element.accepts(v))
            }
            (Self::Data { id, .. }, Value::Data(data)) => *id == data.type_id(),
            _ => false,
        }
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::String(v.into())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Self::String(v.into())
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Self::Integer(v)
    }
}
impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Self::Integer(i64::from(v))
    }
}
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Boolean(v)
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Self::Number(v)
    }
}
impl From<Reference> for Value {
    fn from(v: Reference) -> Self {
        Self::Reference(v)
    }
}
impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Self {
        Self::List(v.into())
    }
}

impl From<LuauValue> for Value {
    fn from(value: LuauValue) -> Self {
        match value {
            LuauValue::Nil => Self::Nil,
            LuauValue::Boolean(value) => Self::Boolean(value),
            LuauValue::Integer(value) => Self::Integer(value),
            LuauValue::Number(value) => Self::Number(value),
            LuauValue::String(value) => Self::from(value),
            LuauValue::Sequence(values) => Self::List(values.into_iter().map(Self::from).collect()),
            LuauValue::Record(values) => Self::Record(Arc::new(
                values
                    .into_iter()
                    .map(|(key, value)| (key, Self::from(value)))
                    .collect(),
            )),
        }
    }
}
