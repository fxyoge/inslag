use crate::{LootError, SourceLocation, Value};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug)]
pub(crate) enum Expression {
    Literal(Value),
    Variable(String),
    Range(i64, i64),
    Not(String),
}

impl Expression {
    pub fn variable(&self) -> Option<&str> {
        match self {
            Self::Variable(name) | Self::Not(name) => Some(name),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub location: SourceLocation,
    pub kind: EntryKind,
    pub binding: Option<String>,
    pub condition: Option<Expression>,
    pub weight: Expression,
}

#[derive(Clone, Debug)]
pub(crate) enum EntryKind {
    Value(Expression),
    All(Arc<[Entry]>),
    Choose {
        entries: Arc<[Entry]>,
        rolls: Expression,
        distinct: bool,
    },
    Call {
        function: String,
        arguments: BTreeMap<String, Expression>,
    },
    Table {
        table: String,
        arguments: BTreeMap<String, Expression>,
    },
    Implicit {
        primary: Option<Expression>,
        arguments: BTreeMap<String, Expression>,
    },
    Nothing,
}

#[derive(Clone, Debug)]
pub(crate) struct Table {
    pub name: String,
    pub location: SourceLocation,
    pub using: Option<String>,
    pub defaults: BTreeMap<String, Expression>,
    pub entries: Arc<[Entry]>,
}

struct Line {
    indent: usize,
    location: SourceLocation,
    tokens: Vec<String>,
}

pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.split('.').all(valid_variable)
}

pub(crate) fn valid_variable(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

fn tokens(text: &str, location: &SourceLocation) -> Result<Vec<String>, LootError> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut escape = false;
    for c in text.chars() {
        if let Some(end) = quote {
            token.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == end {
                quote = None;
            }
        } else {
            match c {
                ';' => break,
                ' ' => {
                    if !token.is_empty() {
                        tokens.push(std::mem::take(&mut token));
                    }
                }
                '\'' | '"' => {
                    quote = Some(c);
                    token.push(c);
                }
                c if c.is_whitespace() => {
                    return Err(LootError::new(
                        location.clone(),
                        "only ASCII spaces are allowed outside quoted values",
                    ));
                }
                _ => token.push(c),
            }
        }
    }
    if quote.is_some() {
        return Err(LootError::new(
            location.clone(),
            "unterminated quoted value",
        ));
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    Ok(tokens)
}

fn unquote(text: &str, location: &SourceLocation) -> Result<String, LootError> {
    let quote = text.chars().next().unwrap_or(' ');
    if !matches!(quote, '\'' | '"') || !text.ends_with(quote) || text.len() < 2 {
        return Err(LootError::new(location.clone(), "expected a quoted value"));
    }
    let mut result = String::new();
    let mut chars = text[1..text.len() - 1].chars();
    while let Some(c) = chars.next() {
        if c == quote {
            return Err(LootError::new(
                location.clone(),
                "unexpected text after quoted value",
            ));
        }
        if c == '\\' {
            let c = match chars.next() {
                Some('n') => '\n',
                Some('t') => '\t',
                Some('r') => '\r',
                Some(c @ ('\\' | '\'' | '"')) => c,
                _ => {
                    return Err(LootError::new(
                        location.clone(),
                        "invalid escape in quoted value",
                    ));
                }
            };
            result.push(c);
        } else {
            result.push(c);
        }
    }
    Ok(result)
}

fn expression(text: &str, location: &SourceLocation) -> Result<Expression, LootError> {
    let fail = |message| LootError::new(location.clone(), message);
    if text.is_empty() {
        return Err(fail("missing argument value"));
    }
    if text.starts_with(['\'', '"']) {
        return Ok(Expression::Literal(Value::from(unquote(text, location)?)));
    }
    if let Some(name) = text.strip_prefix("!$") {
        if !valid_variable(name) {
            return Err(fail("invalid variable name"));
        }
        return Ok(Expression::Not(name.into()));
    }
    if let Some(name) = text.strip_prefix('$') {
        if !valid_variable(name) {
            return Err(fail(
                "invalid variable name; property traversal is not supported",
            ));
        }
        return Ok(Expression::Variable(name.into()));
    }
    if let Some((min, max)) = text.split_once("..") {
        let (Ok(min), Ok(max)) = (min.parse::<i64>(), max.parse::<i64>()) else {
            return Err(fail("ranges require integer endpoints"));
        };
        if min > max {
            return Err(fail("range minimum exceeds maximum"));
        }
        return Ok(Expression::Range(min, max));
    }
    match text {
        "true" => return Ok(Expression::Literal(Value::Boolean(true))),
        "false" => return Ok(Expression::Literal(Value::Boolean(false))),
        _ => {}
    }
    if let Ok(value) = text.parse::<i64>() {
        return Ok(Expression::Literal(Value::Integer(value)));
    }
    if let Ok(value) = text.parse::<f64>() {
        if !value.is_finite() {
            return Err(fail("numbers must be finite"));
        }
        return Ok(Expression::Literal(Value::Number(value)));
    }
    if !valid_name(text) {
        return Err(fail(
            "invalid unquoted value; quote strings containing punctuation or spaces",
        ));
    }
    Ok(Expression::Literal(Value::from(text)))
}

fn attributes(
    tokens: &[String],
    location: &SourceLocation,
) -> Result<BTreeMap<String, String>, LootError> {
    let mut result = BTreeMap::new();
    for token in tokens {
        let Some((key, value)) = token.split_once('=') else {
            return Err(LootError::new(
                location.clone(),
                format!("expected name=value, got {token:?}"),
            ));
        };
        if !valid_variable(key) || value.is_empty() {
            return Err(LootError::new(location.clone(), "invalid attribute"));
        }
        if result.insert(key.into(), value.into()).is_some() {
            return Err(LootError::new(
                location.clone(),
                format!("duplicate attribute {key:?}"),
            ));
        }
    }
    Ok(result)
}

fn expressions(
    attributes: BTreeMap<String, String>,
    location: &SourceLocation,
) -> Result<BTreeMap<String, Expression>, LootError> {
    attributes
        .into_iter()
        .map(|(name, value)| {
            if crate::function::RESERVED.contains(&name.as_str()) {
                return Err(LootError::new(
                    location.clone(),
                    format!("reserved attribute {name:?} is not valid here"),
                ));
            }
            Ok((name, expression(&value, location)?))
        })
        .collect()
}

pub(crate) fn parse(source: &str, text: &str) -> Result<Vec<Table>, LootError> {
    let mut lines = Vec::new();
    for (index, text) in text.lines().enumerate() {
        let indent = text.bytes().take_while(|&c| c == b' ').count();
        let location = SourceLocation {
            source: source.into(),
            line: index + 1,
            column: indent + 1,
        };
        let tokens = tokens(&text[indent..], &location)?;
        if !tokens.is_empty() {
            lines.push(Line {
                indent,
                location,
                tokens,
            });
        }
    }
    let mut parser = Parser { lines, cursor: 0 };
    let mut tables = Vec::new();
    while parser.cursor < parser.lines.len() {
        tables.push(parser.table()?);
    }
    if tables.is_empty() {
        return Err(LootError::new(
            SourceLocation {
                source: source.into(),
                line: 1,
                column: 1,
            },
            "loot file must declare a table",
        ));
    }
    Ok(tables)
}

struct Parser {
    lines: Vec<Line>,
    cursor: usize,
}

impl Parser {
    fn table(&mut self) -> Result<Table, LootError> {
        let line = &self.lines[self.cursor];
        if line.indent != 0
            || line.tokens.first().map(String::as_str) != Some("table")
            || line.tokens.len() < 2
        {
            return Err(LootError::new(
                line.location.clone(),
                "expected table \"name\" at column 1",
            ));
        }
        if !line.tokens[1].starts_with('"') {
            return Err(LootError::new(
                line.location.clone(),
                "table names must be double quoted",
            ));
        }
        let name = unquote(&line.tokens[1], &line.location)?;
        if !valid_name(&name) {
            return Err(LootError::new(line.location.clone(), "invalid table name"));
        }
        let location = line.location.clone();
        let mut attributes = attributes(&line.tokens[2..], &location)?;
        let using = attributes.remove("using");
        if using.as_ref().is_some_and(|name| !valid_name(name)) {
            return Err(LootError::new(location.clone(), "invalid default function"));
        }
        let defaults = expressions(attributes, &location)?;
        if using.is_none() && !defaults.is_empty() {
            return Err(LootError::new(
                location.clone(),
                "header arguments require using=function",
            ));
        }
        self.cursor += 1;
        let entries = if self
            .lines
            .get(self.cursor)
            .is_some_and(|line| line.indent > 0)
        {
            self.block(self.lines[self.cursor].indent, 0, false)?
        } else {
            Vec::new()
        };
        Ok(Table {
            name,
            location,
            using,
            defaults,
            entries: entries.into(),
        })
    }

    fn block(
        &mut self,
        indent: usize,
        depth: usize,
        weighted: bool,
    ) -> Result<Vec<Entry>, LootError> {
        if depth >= 64 {
            return Err(LootError::new(
                self.lines[self.cursor].location.clone(),
                "nesting exceeds 64 levels",
            ));
        }
        let mut result = Vec::new();
        while let Some(line) = self.lines.get(self.cursor) {
            if line.indent < indent {
                break;
            }
            if line.indent != indent {
                return Err(LootError::new(
                    line.location.clone(),
                    "inconsistent block indentation",
                ));
            }
            result.push(self.entry(indent, depth, weighted)?);
        }
        Ok(result)
    }

    fn entry(&mut self, indent: usize, depth: usize, weighted: bool) -> Result<Entry, LootError> {
        let line = &self.lines[self.cursor];
        let location = line.location.clone();
        let first = line.tokens[0].clone();
        let starts_with_argument = !first.starts_with(['\'', '"', '@', '#']) && first.contains('=');
        let mut attributes = attributes(
            &line.tokens[usize::from(!starts_with_argument)..],
            &location,
        )?;
        let binding = attributes.remove("as");
        if binding.as_ref().is_some_and(|name| !valid_variable(name)) {
            return Err(LootError::new(location.clone(), "invalid binding name"));
        }
        let condition = attributes
            .remove("when")
            .map(|value| expression(&value, &location))
            .transpose()?;
        let weight = match attributes.remove("weight") {
            Some(value) if weighted => expression(&value, &location)?,
            Some(_) => {
                return Err(LootError::new(
                    location.clone(),
                    "weight is only allowed on direct children of choose",
                ));
            }
            None => Expression::Literal(Value::Integer(1)),
        };
        let rolls = attributes.remove("rolls");
        let distinct = attributes.remove("distinct");
        self.cursor += 1;
        let children = if self
            .lines
            .get(self.cursor)
            .is_some_and(|line| line.indent > indent)
        {
            self.block(self.lines[self.cursor].indent, depth + 1, first == "choose")?
        } else {
            Vec::new()
        };
        if first != "choose" && (rolls.is_some() || distinct.is_some()) {
            return Err(LootError::new(
                location.clone(),
                "rolls and distinct are only allowed on choose",
            ));
        }
        if !matches!(first.as_str(), "choose" | "all") && !children.is_empty() {
            return Err(LootError::new(
                location.clone(),
                "only choose and all accept child entries",
            ));
        }
        let kind = match first.as_str() {
            "all" | "choose" => {
                if children.is_empty() {
                    return Err(LootError::new(
                        location.clone(),
                        "groups require child entries",
                    ));
                }
                if !attributes.is_empty() {
                    return Err(LootError::new(location.clone(), "unknown group attribute"));
                }
                if first == "all" {
                    EntryKind::All(children.into())
                } else {
                    let rolls = expression(rolls.as_deref().unwrap_or("1"), &location)?;
                    let distinct = match distinct.as_deref() {
                        None | Some("false") => false,
                        Some("true") => true,
                        _ => {
                            return Err(LootError::new(
                                location.clone(),
                                "distinct requires true or false",
                            ));
                        }
                    };
                    EntryKind::Choose {
                        entries: children.into(),
                        rolls,
                        distinct,
                    }
                }
            }
            "nothing" => {
                if !attributes.is_empty() {
                    return Err(LootError::new(
                        location.clone(),
                        "nothing does not accept arguments",
                    ));
                }
                EntryKind::Nothing
            }
            _ if first.starts_with('@') || first.starts_with('#') => {
                let name = &first[1..];
                if !valid_name(name) {
                    return Err(LootError::new(
                        location.clone(),
                        "invalid function or table reference",
                    ));
                }
                let arguments = expressions(attributes, &location)?;
                if first.starts_with('@') {
                    EntryKind::Call {
                        function: name.into(),
                        arguments,
                    }
                } else {
                    EntryKind::Table {
                        table: name.into(),
                        arguments,
                    }
                }
            }
            _ => EntryKind::Implicit {
                primary: if starts_with_argument {
                    None
                } else {
                    Some(expression(&first, &location)?)
                },
                arguments: expressions(attributes, &location)?,
            },
        };
        Ok(Entry {
            location,
            kind,
            binding,
            condition,
            weight,
        })
    }
}
