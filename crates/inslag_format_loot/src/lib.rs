//! Owned syntax and byte-spanned errors for `.loot` files, without runtime linking or execution.

#![warn(missing_docs)]

mod error;
mod parser;
mod syntax;

pub use error::{ErrorKind, ParseError};
pub use parser::{is_name, is_reserved, is_variable, parse};
pub use syntax::{
    Argument, Document, Entry, EntryKind, Expression, ExpressionKind, Literal, Span, Spanned, Table,
};

impl std::str::FromStr for Document {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        parse(input)
    }
}
