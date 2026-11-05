use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLocation {
    pub source: Arc<str>,
    pub line: usize,
    pub column: usize,
}

impl std::fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.source, self.line, self.column)
    }
}

pub(crate) struct SourceMap<'a> {
    name: Arc<str>,
    text: &'a str,
    lines: Vec<usize>,
}

impl<'a> SourceMap<'a> {
    pub fn new(name: &str, text: &'a str) -> Self {
        Self {
            name: name.into(),
            text,
            lines: std::iter::once(0)
                .chain(text.match_indices('\n').map(|(index, _)| index + 1))
                .collect(),
        }
    }

    pub fn location(&self, offset: usize) -> SourceLocation {
        let line = self.lines.partition_point(|&start| start <= offset) - 1;
        SourceLocation {
            source: self.name.clone(),
            line: line + 1,
            column: self.text[self.lines[line]..offset].chars().count() + 1,
        }
    }
}

/// A loot failure whose display text includes its source location.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{location}: {message}")]
pub struct LootError {
    pub(crate) location: SourceLocation,
    pub(crate) message: String,
    pub(crate) table_path: Vec<String>,
}

impl LootError {
    pub(crate) fn new(location: SourceLocation, message: impl Into<String>) -> Self {
        Self {
            location,
            message: message.into(),
            table_path: Vec::new(),
        }
    }

    pub(crate) fn request(message: impl Into<String>) -> Self {
        Self::new(
            SourceLocation {
                source: "<request>".into(),
                line: 1,
                column: 1,
            },
            message,
        )
    }
}
