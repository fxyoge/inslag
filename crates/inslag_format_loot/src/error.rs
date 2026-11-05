use crate::Span;

/// The first syntax error, independent of filenames or diagnostic rendering.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{kind} at bytes {}..{}", span.start, span.end)]
pub struct ParseError {
    /// Machine-readable failure category.
    pub kind: ErrorKind,
    /// Offending source range, or an empty range where input was expected.
    pub span: Span,
}

impl ParseError {
    pub(crate) fn new(kind: ErrorKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// Syntax failures; unresolved names and runtime type errors are not included.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ErrorKind {
    /// No table declaration remains after ignoring comments and blank lines.
    #[error("loot file must declare a table")]
    EmptyDocument,
    /// A top-level declaration lacks `table`, a name, or column-one alignment.
    #[error("expected table \"name\" at column 1")]
    ExpectedTable,
    /// A table name is not double quoted.
    #[error("table names must be double quoted")]
    ExpectedQuotedTableName,
    /// A decoded table name is not a dot-separated sequence of identifiers.
    #[error("invalid table name")]
    InvalidTableName,
    /// A `using` value is not a valid unquoted function name.
    #[error("invalid default function")]
    InvalidDefaultFunction,
    /// An `@` or `#` prefix is not followed by a valid name.
    #[error("invalid function or table reference")]
    InvalidReference,
    /// A variable reference has an invalid name or attempts property traversal.
    #[error("invalid variable name; property traversal is not supported")]
    InvalidVariable,
    /// An `as` value is not a valid variable name.
    #[error("invalid binding name")]
    InvalidBinding,
    /// An unquoted expression is neither a supported scalar nor a valid name.
    #[error("invalid unquoted value; quote strings containing punctuation or spaces")]
    InvalidValue,
    /// A named argument lacks `=`.
    #[error("expected name=value")]
    ExpectedAttribute,
    /// The name before `=` is not a valid identifier.
    #[error("invalid attribute")]
    InvalidAttribute,
    /// The same attribute name occurs more than once on a line.
    #[error("duplicate attribute {0:?}")]
    DuplicateAttribute(String),
    /// A language keyword is used as an ordinary argument name.
    #[error("reserved attribute {0:?} is not valid here")]
    ReservedAttribute(String),
    /// No expression follows `=`.
    #[error("missing argument value")]
    MissingValue,
    /// A quoted value has no closing quote on its line.
    #[error("unterminated quoted value")]
    UnterminatedString,
    /// A quoted expression has a suffix without a separating space.
    #[error("unexpected text after quoted value")]
    UnexpectedAfterString,
    /// A backslash introduces an unsupported string escape.
    #[error("invalid escape in quoted value")]
    InvalidEscape,
    /// Whitespace other than ASCII space occurs outside quotes or comments.
    #[error("only ASCII spaces are allowed outside quoted values")]
    InvalidWhitespace,
    /// A range endpoint cannot be represented as a signed 64-bit integer.
    #[error("ranges require integer endpoints")]
    InvalidRange,
    /// A range's lower endpoint exceeds its upper endpoint.
    #[error("range minimum exceeds maximum")]
    ReversedRange,
    /// A numeric literal resolves to infinity or NaN.
    #[error("numbers must be finite")]
    NonFiniteNumber,
    /// An entry's indentation does not match its enclosing block.
    #[error("inconsistent block indentation")]
    InconsistentIndentation,
    /// The file exceeds the parser's 64-level block limit.
    #[error("nesting exceeds 64 levels")]
    NestingLimit,
    /// An `all` or `choose` group has no children.
    #[error("groups require child entries")]
    MissingChildren,
    /// An entry other than `all` or `choose` has children.
    #[error("only choose and all accept child entries")]
    UnexpectedChildren,
    /// A group has an attribute outside its supported set.
    #[error("unknown group attribute")]
    UnknownGroupAttribute,
    /// A `nothing` entry has ordinary named arguments.
    #[error("nothing does not accept arguments")]
    UnexpectedArguments,
    /// A `weight` attribute is not on a direct child of `choose`.
    #[error("weight is only allowed on direct children of choose")]
    WeightOutsideChoose,
    /// Choice controls cannot apply to other entry kinds.
    #[error("rolls, distinct, and exclude are only allowed on choose")]
    ChooseAttributeOutsideChoose,
    /// A `distinct` value is not the unquoted keyword `true` or `false`.
    #[error("distinct requires true or false")]
    InvalidDistinct,
    /// Table-header arguments are present without `using`.
    #[error("header arguments require using=function")]
    HeaderArgumentsWithoutUsing,
}
