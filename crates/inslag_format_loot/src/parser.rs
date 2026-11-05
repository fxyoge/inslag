use crate::{
    Argument, Document, Entry, EntryKind, ErrorKind, Expression, ExpressionKind, Literal,
    ParseError, Span, Spanned, Table,
};
use std::collections::BTreeSet;
use winnow::{
    ModalResult, Parser,
    ascii::escaped,
    combinator::{alt, delimited, preceded, repeat, separated},
    stream::LocatingSlice,
    token::{any, one_of, take_till, take_while},
};

type Input<'a> = LocatingSlice<&'a str>;

/// Parses a complete file, returning its first syntax error with a UTF-8 byte span.
/// Function names, table references, and expression types are not resolved or validated.
pub fn parse(input: &str) -> Result<Document, ParseError> {
    let mut lines = Vec::new();
    let mut offset = 0;
    for raw in input.split_inclusive('\n') {
        let text = raw.strip_suffix('\n').unwrap_or(raw);
        let text = if raw.ends_with('\n') {
            text.strip_suffix('\r').unwrap_or(text)
        } else {
            text
        };
        let tokens = tokenize(text, offset)?;
        if let Some(first) = tokens.first() {
            lines.push(Line {
                indent: first.span.start - offset,
                span: first.span.start..tokens.last().expect("nonempty tokens").span.end,
                tokens,
            });
        }
        offset += raw.len();
    }
    let mut parser = DocumentParser {
        lines: lines.into_iter().peekable(),
    };
    let mut tables = Vec::new();
    while parser.lines.peek().is_some() {
        tables.push(parser.table()?);
    }
    if tables.is_empty() {
        return Err(ParseError::new(ErrorKind::EmptyDocument, 0..0));
    }
    Ok(Document { tables })
}

#[derive(Clone)]
struct Token<'a> {
    text: &'a str,
    span: Span,
}

fn quoted(input: &mut Input<'_>) -> ModalResult<()> {
    let quote = one_of(['\'', '"']).parse_next(input)?;
    let _: () = repeat(
        0..,
        alt((
            take_till(1.., move |c| c == quote || c == '\\').void(),
            preceded('\\', any).void(),
        )),
    )
    .parse_next(input)?;
    quote.void().parse_next(input)
}

fn lexeme<'a>(input: &mut Input<'a>) -> ModalResult<&'a str> {
    repeat::<_, _, (), _, _>(
        1..,
        alt((
            quoted,
            take_while(1.., |c: char| {
                !c.is_whitespace() && !matches!(c, '\'' | '"' | ';')
            })
            .void(),
        )),
    )
    .take()
    .parse_next(input)
}

fn tokenize(text: &str, offset: usize) -> Result<Vec<Token<'_>>, ParseError> {
    let mut input = Input::new(text);
    let mut tokens = Vec::new();
    while !input.is_empty() {
        let _: &str = take_while::<_, _, winnow::error::ContextError>(0.., ' ')
            .parse_next(&mut input)
            .expect("zero or more spaces");
        if input.is_empty() || input.starts_with(';') {
            break;
        }
        let start = text.len() - input.len();
        if input.chars().next().is_some_and(char::is_whitespace) {
            let end = start + input.chars().next().expect("nonempty input").len_utf8();
            return Err(ParseError::new(
                ErrorKind::InvalidWhitespace,
                offset + start..offset + end,
            ));
        }
        let (value, span) = lexeme.with_span().parse_next(&mut input).map_err(|_| {
            ParseError::new(
                ErrorKind::UnterminatedString,
                offset + start..offset + text.len(),
            )
        })?;
        tokens.push(Token {
            text: value,
            span: offset + span.start..offset + span.end,
        });
    }
    Ok(tokens)
}

struct Attribute<'a> {
    name: Spanned<String>,
    value: Token<'a>,
    span: Span,
}

fn attributes<'a>(
    tokens: impl Iterator<Item = Token<'a>>,
) -> Result<Vec<Attribute<'a>>, ParseError> {
    let mut names = BTreeSet::new();
    let mut result = Vec::new();
    for token in tokens {
        let Some((key, value)) = token.text.split_once('=') else {
            return Err(ParseError::new(ErrorKind::ExpectedAttribute, token.span));
        };
        let name_span = token.span.start..token.span.start + key.len();
        if !is_variable(key) {
            return Err(ParseError::new(ErrorKind::InvalidAttribute, name_span));
        }
        if value.is_empty() {
            return Err(ParseError::new(
                ErrorKind::MissingValue,
                token.span.end..token.span.end,
            ));
        }
        if !names.insert(key) {
            return Err(ParseError::new(
                ErrorKind::DuplicateAttribute(key.into()),
                name_span,
            ));
        }
        result.push(Attribute {
            name: Spanned {
                value: key.into(),
                span: name_span,
            },
            value: Token {
                text: value,
                span: token.span.start + key.len() + 1..token.span.end,
            },
            span: token.span,
        });
    }
    Ok(result)
}

fn remove<'a>(attributes: &mut Vec<Attribute<'a>>, name: &str) -> Option<Attribute<'a>> {
    let index = attributes.iter().position(|a| a.name.value == name)?;
    Some(attributes.remove(index))
}

fn arguments(attributes: Vec<Attribute<'_>>) -> Result<Vec<Argument>, ParseError> {
    attributes
        .into_iter()
        .map(|attribute| {
            if is_reserved(&attribute.name.value) {
                return Err(ParseError::new(
                    ErrorKind::ReservedAttribute(attribute.name.value),
                    attribute.name.span,
                ));
            }
            Ok(Argument {
                span: attribute.span,
                name: attribute.name,
                value: expression(attribute.value)?,
            })
        })
        .collect()
}

/// Tests whether a name is reserved for a language keyword or control attribute.
pub fn is_reserved(name: &str) -> bool {
    matches!(
        name,
        "table"
            | "choose"
            | "all"
            | "nothing"
            | "using"
            | "as"
            | "weight"
            | "when"
            | "rolls"
            | "distinct"
    )
}

fn expression(token: Token<'_>) -> Result<Expression, ParseError> {
    let text = token.text;
    let fail = |kind| ParseError::new(kind, token.span.clone());
    let value = if text.is_empty() {
        return Err(fail(ErrorKind::MissingValue));
    } else if text.starts_with(['\'', '"']) {
        ExpressionKind::Literal(Literal::String(unquote(&token)?))
    } else if let Some(name) = text.strip_prefix("!$") {
        if !is_variable(name) {
            return Err(fail(ErrorKind::InvalidVariable));
        }
        ExpressionKind::Not(name.into())
    } else if let Some(name) = text.strip_prefix('$') {
        if !is_variable(name) {
            return Err(fail(ErrorKind::InvalidVariable));
        }
        ExpressionKind::Variable(name.into())
    } else if let Some((min, max)) = text.split_once("..") {
        let (Ok(min), Ok(max)) = (min.parse::<i64>(), max.parse::<i64>()) else {
            return Err(fail(ErrorKind::InvalidRange));
        };
        if min > max {
            return Err(fail(ErrorKind::ReversedRange));
        }
        ExpressionKind::Range(min, max)
    } else if let Ok(value) = boolean.parse(text) {
        ExpressionKind::Literal(Literal::Boolean(value))
    } else if let Ok(value) = text.parse::<i64>() {
        ExpressionKind::Literal(Literal::Integer(value))
    } else if let Ok(value) = text.parse::<f64>() {
        if !value.is_finite() {
            return Err(fail(ErrorKind::NonFiniteNumber));
        }
        ExpressionKind::Literal(Literal::Number(value))
    } else {
        if !is_name(text) {
            return Err(fail(ErrorKind::InvalidValue));
        }
        ExpressionKind::Literal(Literal::String(text.into()))
    };
    Ok(Spanned {
        value,
        span: token.span,
    })
}

fn unquote(token: &Token<'_>) -> Result<String, ParseError> {
    let quote = token.text.chars().next().expect("nonempty token");
    let mut input = token.text;
    let result: ModalResult<String> = delimited(
        quote,
        escaped(
            take_till(1.., move |c| c == quote || c == '\\'),
            '\\',
            one_of(['n', 't', 'r', '\\', '\'', '"']).map(|c| match c {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                c => c,
            }),
        ),
        quote,
    )
    .parse_next(&mut input);
    let offset = token.text.len() - input.len();
    let end = offset + input.chars().next().map_or(0, char::len_utf8);
    match result {
        Ok(value) if input.is_empty() => Ok(value),
        Ok(_) => Err(ParseError::new(
            ErrorKind::UnexpectedAfterString,
            token.span.start + offset..token.span.start + end,
        )),
        Err(_) => Err(ParseError::new(
            ErrorKind::InvalidEscape,
            token.span.start + offset.saturating_sub(1)..token.span.start + end,
        )),
    }
}

/// Tests an ASCII variable or attribute name, without `$`, `!$`, or property traversal.
/// The first character must be a letter or `_`; later characters may also be digits or `-`.
pub fn is_variable(name: &str) -> bool {
    identifier.parse(name).is_ok()
}

/// Tests a nonempty dot-separated sequence of variable names, as used for tables and functions.
/// Reserved keywords are accepted; use [`is_reserved`] when that restriction also applies.
pub fn is_name(name: &str) -> bool {
    separated::<_, _, (), _, _, _, _>(1.., identifier, '.')
        .parse(name)
        .is_ok()
}

fn identifier<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    (
        one_of(|c: char| c.is_ascii_alphabetic() || c == '_'),
        take_while(0.., |c: char| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '-')
        }),
    )
        .take()
        .parse_next(input)
}

fn boolean(input: &mut &str) -> ModalResult<bool> {
    alt(("true".value(true), "false".value(false))).parse_next(input)
}

struct Line<'a> {
    indent: usize,
    span: Span,
    tokens: Vec<Token<'a>>,
}

struct DocumentParser<'a> {
    lines: std::iter::Peekable<std::vec::IntoIter<Line<'a>>>,
}

impl DocumentParser<'_> {
    fn table(&mut self) -> Result<Table, ParseError> {
        let line = self.lines.next().expect("table line exists");
        if line.indent != 0 || line.tokens[0].text != "table" || line.tokens.len() < 2 {
            return Err(ParseError::new(ErrorKind::ExpectedTable, line.span));
        }
        let mut tokens = line.tokens.into_iter().skip(1);
        let name = tokens.next().expect("table name exists");
        if !name.text.starts_with('"') {
            return Err(ParseError::new(
                ErrorKind::ExpectedQuotedTableName,
                name.span,
            ));
        }
        let name = Spanned {
            value: unquote(&name)?,
            span: name.span,
        };
        if !is_name(&name.value) {
            return Err(ParseError::new(ErrorKind::InvalidTableName, name.span));
        }
        let mut attrs = attributes(tokens)?;
        let using = remove(&mut attrs, "using")
            .map(|attribute| {
                if !is_name(attribute.value.text) {
                    return Err(ParseError::new(
                        ErrorKind::InvalidDefaultFunction,
                        attribute.value.span,
                    ));
                }
                Ok(Spanned {
                    value: attribute.value.text.into(),
                    span: attribute.value.span,
                })
            })
            .transpose()?;
        let defaults = arguments(attrs)?;
        if using.is_none()
            && let Some(argument) = defaults.first()
        {
            return Err(ParseError::new(
                ErrorKind::HeaderArgumentsWithoutUsing,
                argument.span.clone(),
            ));
        }
        let entries = self.children(0, 0, false)?;
        let end = entries.last().map_or(line.span.end, |entry| entry.span.end);
        Ok(Table {
            span: line.span.start..end,
            name,
            using,
            defaults,
            entries,
        })
    }

    fn children(
        &mut self,
        parent_indent: usize,
        depth: usize,
        weighted: bool,
    ) -> Result<Vec<Entry>, ParseError> {
        let Some(line) = self.lines.peek() else {
            return Ok(Vec::new());
        };
        if line.indent <= parent_indent {
            return Ok(Vec::new());
        }
        if depth >= 64 {
            return Err(ParseError::new(ErrorKind::NestingLimit, line.span.clone()));
        }
        let indent = line.indent;
        let mut entries = Vec::new();
        while let Some(line) = self.lines.peek() {
            if line.indent < indent {
                break;
            }
            if line.indent != indent {
                return Err(ParseError::new(
                    ErrorKind::InconsistentIndentation,
                    line.span.clone(),
                ));
            }
            entries.push(self.entry(depth, weighted)?);
        }
        Ok(entries)
    }

    fn entry(&mut self, depth: usize, weighted: bool) -> Result<Entry, ParseError> {
        let line = self.lines.next().expect("entry line exists");
        let mut tokens = line.tokens.into_iter();
        let first = tokens.next().expect("entry token exists");
        let starts_with_argument =
            !first.text.starts_with(['\'', '"', '@', '#']) && first.text.contains('=');
        let mut attrs = attributes(
            starts_with_argument
                .then(|| first.clone())
                .into_iter()
                .chain(tokens),
        )?;
        let binding = remove(&mut attrs, "as")
            .map(|attribute| {
                if !is_variable(attribute.value.text) {
                    return Err(ParseError::new(
                        ErrorKind::InvalidBinding,
                        attribute.value.span,
                    ));
                }
                Ok(Spanned {
                    value: attribute.value.text.into(),
                    span: attribute.value.span,
                })
            })
            .transpose()?;
        let condition = remove(&mut attrs, "when")
            .map(|attribute| {
                let token = attribute.value;
                let value = if token.text.starts_with(['\'', '"']) {
                    unquote(&token)?
                } else {
                    token.text.to_owned()
                };
                Ok(Spanned {
                    value,
                    span: token.span,
                })
            })
            .transpose()?;
        let weight = remove(&mut attrs, "weight")
            .map(|attribute| {
                if !weighted {
                    return Err(ParseError::new(
                        ErrorKind::WeightOutsideChoose,
                        attribute.name.span,
                    ));
                }
                expression(attribute.value)
            })
            .transpose()?;
        let rolls = remove(&mut attrs, "rolls");
        let distinct = remove(&mut attrs, "distinct");
        let children = self.children(line.indent, depth + 1, first.text == "choose")?;
        if first.text != "choose"
            && let Some(attribute) = rolls.as_ref().or(distinct.as_ref())
        {
            return Err(ParseError::new(
                ErrorKind::ChooseAttributeOutsideChoose,
                attribute.name.span.clone(),
            ));
        }
        if !matches!(first.text, "choose" | "all") && !children.is_empty() {
            return Err(ParseError::new(
                ErrorKind::UnexpectedChildren,
                children[0].span.clone(),
            ));
        }
        let end = children
            .last()
            .map_or(line.span.end, |entry| entry.span.end);
        let kind = match first.text {
            "all" | "choose" => {
                if children.is_empty() {
                    return Err(ParseError::new(ErrorKind::MissingChildren, first.span));
                }
                if let Some(attribute) = attrs.first() {
                    return Err(ParseError::new(
                        ErrorKind::UnknownGroupAttribute,
                        attribute.name.span.clone(),
                    ));
                }
                if first.text == "all" {
                    EntryKind::All(children)
                } else {
                    let rolls = rolls
                        .map(|attribute| expression(attribute.value))
                        .transpose()?;
                    let distinct = distinct
                        .map(|attribute| {
                            let value = match attribute.value.text {
                                "true" => true,
                                "false" => false,
                                _ => {
                                    return Err(ParseError::new(
                                        ErrorKind::InvalidDistinct,
                                        attribute.value.span,
                                    ));
                                }
                            };
                            Ok(Spanned {
                                value,
                                span: attribute.value.span,
                            })
                        })
                        .transpose()?;
                    EntryKind::Choose {
                        entries: children,
                        rolls,
                        distinct,
                    }
                }
            }
            "nothing" => {
                if let Some(attribute) = attrs.first() {
                    return Err(ParseError::new(
                        ErrorKind::UnexpectedArguments,
                        attribute.name.span.clone(),
                    ));
                }
                EntryKind::Nothing
            }
            text if text.starts_with('@') || text.starts_with('#') => {
                let name = &text[1..];
                if !is_name(name) {
                    return Err(ParseError::new(ErrorKind::InvalidReference, first.span));
                }
                let name = Spanned {
                    value: name.into(),
                    span: first.span.start + 1..first.span.end,
                };
                let arguments = arguments(attrs)?;
                if text.starts_with('@') {
                    EntryKind::Call {
                        function: name,
                        arguments,
                    }
                } else {
                    EntryKind::Table {
                        table: name,
                        arguments,
                    }
                }
            }
            _ => EntryKind::Implicit {
                primary: if starts_with_argument {
                    None
                } else {
                    Some(expression(first)?)
                },
                arguments: arguments(attrs)?,
            },
        };
        Ok(Entry {
            span: line.span.start..end,
            kind,
            binding,
            condition,
            weight,
        })
    }
}
