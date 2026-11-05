use crate::{FunctionSignatures, SourceLocation, Value};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug)]
pub(crate) struct ContextExpression {
    pub expression: Arc<inslag_luau::CompiledLuauExpression>,
    pub inputs: Vec<String>,
    pub location: SourceLocation,
    pub attribute: &'static str,
}

#[derive(Clone, Debug)]
pub(crate) enum Expression {
    Literal(Value),
    Variable(String),
    Range(i64, i64),
    Not(String),
}

impl Expression {
    pub(crate) fn variable(&self) -> Option<&str> {
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
    pub condition: Option<ContextExpression>,
    pub weight: Expression,
}

impl Entry {
    pub(crate) fn selection_value(&self, functions: &FunctionSignatures) -> Option<&Expression> {
        match &self.kind {
            EntryKind::Value(value) => Some(value),
            EntryKind::Call {
                function,
                arguments,
            } => {
                let primary = functions.get(function)?.primary.as_ref()?;
                arguments.get(primary)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum EntryKind {
    Value(Expression),
    All(Arc<[Entry]>),
    Choose {
        entries: Arc<[Entry]>,
        rolls: Expression,
        distinct: bool,
        exclude: Option<ContextExpression>,
    },
    Call {
        function: String,
        arguments: BTreeMap<String, Expression>,
    },
    Table {
        table: String,
        arguments: BTreeMap<String, Expression>,
    },
    Nothing,
}

#[derive(Clone, Debug)]
pub(crate) struct Table {
    pub name: String,
    pub location: SourceLocation,
    pub entries: Arc<[Entry]>,
}
