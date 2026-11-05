/// A half-open UTF-8 byte range into the original input; empty ranges mark insertion points.
pub type Span = std::ops::Range<usize>;

/// A decoded value paired with the range of its original spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spanned<T> {
    /// Decoded syntax, which need not match the source spelling.
    pub value: T,
    /// Original spelling, including quotes or expression prefixes when present.
    pub span: Span,
}

/// One parsed file; names and references have not been linked or type-checked.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    /// Table declarations in source order, including any duplicate names.
    pub tables: Vec<Table>,
}

/// A `table "name"` declaration and its indented body.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    /// Header through the last descendant, excluding trailing whitespace and comments.
    pub span: Span,
    /// Decoded table name; its span includes the double quotes.
    pub name: Spanned<String>,
    /// Default function named by `using`, if authored.
    pub using: Option<Spanned<String>>,
    /// Default function arguments in source order, without runtime defaults applied.
    pub defaults: Vec<Argument>,
    /// Direct children in source order.
    pub entries: Vec<Entry>,
}

/// An authored entry, before implicit calls and omitted attributes are resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Entry through its last descendant, excluding trailing whitespace and comments.
    pub span: Span,
    /// Entry syntax, independent of its shared attributes.
    pub kind: EntryKind,
    /// Binding named by `as`, without the attribute prefix.
    pub binding: Option<Spanned<String>>,
    /// Luau source for `when`, decoded without compiling it.
    pub condition: Option<Spanned<String>>,
    /// Authored `weight` expression; absence does not insert a literal `1`.
    pub weight: Option<Expression>,
}

/// Entry forms recognized without consulting a function registry or other files.
#[derive(Clone, Debug, PartialEq)]
pub enum EntryKind {
    /// An `all` group with children in source order.
    All(Vec<Entry>),
    /// A `choose` group; attribute types and values are not runtime-validated.
    Choose {
        /// Direct candidates in source order.
        entries: Vec<Entry>,
        /// Authored roll count; omission is distinct from `rolls=1`.
        rolls: Option<Expression>,
        /// Authored distinctness flag; omission is distinct from `distinct=false`.
        distinct: Option<Spanned<bool>>,
    },
    /// An explicit `@function` call, whether or not the function exists.
    Call {
        /// Function name; its span excludes `@`.
        function: Spanned<String>,
        /// Named arguments in source order.
        arguments: Vec<Argument>,
    },
    /// A `#table` reference, whether or not the table exists.
    Table {
        /// Referenced name; its span excludes `#`.
        table: Spanned<String>,
        /// Named arguments in source order.
        arguments: Vec<Argument>,
    },
    /// A bare expression or argument list whose meaning depends on the table's `using`.
    Implicit {
        /// Unnamed expression, absent when the entry begins with an attribute.
        primary: Option<Expression>,
        /// Named arguments in source order.
        arguments: Vec<Argument>,
    },
    /// The `nothing` keyword.
    Nothing,
}

/// A non-reserved `name=value` attribute.
#[derive(Clone, Debug, PartialEq)]
pub struct Argument {
    /// Entire attribute, including `=`.
    pub span: Span,
    /// Attribute name, excluding `=`.
    pub name: Spanned<String>,
    /// Value expression, excluding the name and `=`.
    pub value: Expression,
}

/// An expression paired with its full source spelling.
pub type Expression = Spanned<ExpressionKind>;

/// Unevaluated expression syntax; variable lookup and sampling belong to the runtime.
#[derive(Clone, Debug, PartialEq)]
pub enum ExpressionKind {
    /// A decoded scalar value.
    Literal(Literal),
    /// A `$name` lookup; the stored name excludes `$`.
    Variable(String),
    /// An inclusive integer range written `min..max`, with `min <= max`.
    Range(i64, i64),
    /// A `!$name` lookup; the stored name excludes `!$`.
    Not(String),
}

/// Scalar values representable in a `.loot` file, without application-defined data.
#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    /// An unquoted `true` or `false`.
    Boolean(bool),
    /// A signed 64-bit integer.
    Integer(i64),
    /// A finite floating-point number, distinct from an integer literal.
    Number(f64),
    /// Decoded quoted text or a valid unquoted name.
    String(String),
}
