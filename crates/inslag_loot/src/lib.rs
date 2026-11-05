//! Asset-backed random grab bags for Bevy.

#![warn(missing_docs)]

mod bevy;
mod catalog;
mod error;
mod function;
mod program;
mod session;
mod typed;
mod value;

#[cfg(test)]
pub(crate) use bevy::LootRegistry;
pub use bevy::{LootAppExt, LootFinished, LootFunctionContext, LootPlugin, LootRequest, RollId};
#[cfg(test)]
pub(crate) use catalog::LootTable;
pub(crate) use catalog::{Catalog, SourceDocument};
pub use error::LootError;
pub(crate) use error::SourceLocation;
pub(crate) use function::{Arguments, FromLootValue, FunctionSignature, FunctionSignatures};
pub use inslag_loot_macros::{LootArgs, LootOutput, LootValue};
pub(crate) use typed::LootReturn;
pub use typed::{LootArgs, LootOutput, LootValue};
pub use value::Reference;
extern crate self as inslag_loot;
#[cfg(test)]
pub(crate) use session::TraceEvent;
pub(crate) use session::{FunctionCall, Limits, Session, Step};
pub(crate) use value::{Data, Value, ValueType};

#[doc(hidden)]
pub mod __private {
    pub use crate::bevy::LootFunction;
    pub use crate::function::{Arguments, FromLootValue, FunctionSignature};
    pub use crate::typed::LootReturn;
    pub use crate::value::{Value, ValueType};
    pub use crate::{LootArgs, LootOutput, LootValue};
}

/// Common loot imports.
pub mod prelude {
    pub use crate::{
        LootAppExt, LootArgs, LootError, LootFinished, LootFunctionContext, LootOutput, LootPlugin,
        LootRequest, LootValue, Reference, RollId,
    };
}

#[cfg(test)]
#[path = "tests/examples.rs"]
mod example_tests;
#[cfg(test)]
#[path = "tests/generated.rs"]
mod generated_tests;
#[cfg(test)]
#[path = "tests/language.rs"]
mod language_tests;
#[cfg(test)]
#[path = "tests/typed.rs"]
mod typed_tests;
