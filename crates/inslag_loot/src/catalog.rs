use crate::parser::{self, Entry, EntryKind, Expression, Table};
use crate::{FunctionSignature, FunctionSignatures, LootError, LootOutput, Value, ValueType};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct SourceDocument {
    pub name: String,
    pub text: String,
}

impl SourceDocument {
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            text: text.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Catalog {
    pub(crate) tables: BTreeMap<String, Table>,
    pub(crate) inputs: BTreeMap<String, BTreeSet<String>>,
    pub(crate) functions: FunctionSignatures,
}

#[cfg(test)]
#[derive(Debug)]
pub struct LootTable<T> {
    pub(crate) catalog: Arc<Catalog>,
    pub(crate) name: String,
    marker: std::marker::PhantomData<fn() -> T>,
}

#[cfg(test)]
impl<T> Clone for LootTable<T> {
    fn clone(&self) -> Self {
        Self {
            catalog: self.catalog.clone(),
            name: self.name.clone(),
            marker: std::marker::PhantomData,
        }
    }
}

#[cfg(test)]
impl<T> LootTable<T> {
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Catalog {
    #[cfg(test)]
    pub fn table<T: LootOutput>(self: &Arc<Self>, name: &str) -> Result<LootTable<T>, LootError> {
        self.validate_output::<T>(name)?;
        Ok(LootTable {
            catalog: self.clone(),
            name: name.into(),
            marker: std::marker::PhantomData,
        })
    }

    pub(crate) fn validate_output<T: LootOutput>(&self, name: &str) -> Result<(), LootError> {
        let table = self
            .tables
            .get(name)
            .ok_or_else(|| LootError::request(format!("unknown table {name:?}")))?;
        self.validate_output_entries::<T>(&table.entries, &mut BTreeSet::new())
    }

    fn validate_output_entries<T: LootOutput>(
        &self,
        entries: &[Entry],
        visited: &mut BTreeSet<String>,
    ) -> Result<(), LootError> {
        for entry in entries {
            let output = match &entry.kind {
                EntryKind::Call { function, .. } => self
                    .functions
                    .get(function)
                    .expect("linked function exists")
                    .output()
                    .cloned(),
                EntryKind::Value(Expression::Literal(value)) => {
                    if !T::accepts_value(value) {
                        return Err(LootError::new(
                            entry.location.clone(),
                            format!(
                                "output {value:?} is not supported by {}",
                                std::any::type_name::<T>()
                            ),
                        ));
                    }
                    None
                }
                EntryKind::Value(Expression::Range(min, max)) => Some(ValueType::IntegerRange {
                    min: *min,
                    max: *max,
                }),
                EntryKind::Value(_) => Some(ValueType::Any),
                EntryKind::Table { table, .. } => {
                    if visited.insert(table.clone()) {
                        self.validate_output_entries::<T>(&self.tables[table].entries, visited)?;
                    }
                    None
                }
                EntryKind::All(children)
                | EntryKind::Choose {
                    entries: children, ..
                } => {
                    self.validate_output_entries::<T>(children, visited)?;
                    None
                }
                EntryKind::Nothing => None,
                EntryKind::Implicit { .. } => unreachable!("catalog lowers implicit calls"),
            };
            if let Some(output) = output
                && !T::accepts_type(&output)
            {
                return Err(LootError::new(
                    entry.location.clone(),
                    format!(
                        "output {output:?} is not supported by {}",
                        std::any::type_name::<T>()
                    ),
                ));
            }
        }
        Ok(())
    }

    pub fn compile(
        sources: impl IntoIterator<Item = SourceDocument>,
        functions: &FunctionSignatures,
    ) -> Result<Self, LootError> {
        let mut tables = BTreeMap::new();
        for source in sources {
            for mut table in parser::parse(&source.name, &source.text)? {
                if let Some(previous) = tables.get(&table.name) {
                    let previous: &Table = previous;
                    return Err(LootError::new(
                        table.location.clone(),
                        format!(
                            "duplicate table {:?}; first declared at {}",
                            table.name, previous.location
                        ),
                    ));
                }
                if let Some(using) = &table.using {
                    let signature = functions.get(using).ok_or_else(|| {
                        LootError::new(
                            table.location.clone(),
                            format!("unknown default function {using:?}"),
                        )
                    })?;
                    validate_arguments(signature, &table.defaults, false, &table.location)?;
                }
                lower_entries(
                    Arc::make_mut(&mut table.entries),
                    table.using.as_deref(),
                    &table.defaults,
                    functions,
                )?;
                tables.insert(table.name.clone(), table);
            }
        }
        if tables.is_empty() {
            return Err(LootError::request(
                "catalog needs at least one source document",
            ));
        }
        let mut inputs = BTreeMap::new();
        for table in tables.values() {
            let mut declarations = BTreeSet::new();
            collect_bindings(&table.entries, &mut declarations);
            let mut required = BTreeSet::new();
            infer_inputs(
                &table.entries,
                &mut BTreeSet::new(),
                &declarations,
                &mut required,
            )?;
            inputs.insert(table.name.clone(), required);
        }
        let catalog = Self {
            tables,
            inputs,
            functions: functions.clone(),
        };
        for table in catalog.tables.values() {
            catalog.validate_references(&table.entries)?;
        }
        let mut visited = BTreeSet::new();
        for table in catalog.tables.values() {
            catalog.check_cycles(table, &mut Vec::new(), &mut visited)?;
        }
        Ok(catalog)
    }

    #[cfg(test)]
    pub fn required_inputs(&self, table: &str) -> Option<impl Iterator<Item = &str>> {
        self.inputs
            .get(table)
            .map(|names| names.iter().map(String::as_str))
    }

    fn validate_references(&self, entries: &[Entry]) -> Result<(), LootError> {
        for entry in entries {
            match &entry.kind {
                EntryKind::Table { table, arguments } => {
                    let required = self.inputs.get(table).ok_or_else(|| {
                        LootError::new(entry.location.clone(), format!("unknown table {table:?}"))
                    })?;
                    for name in required {
                        if !arguments.contains_key(name) {
                            return Err(LootError::new(
                                entry.location.clone(),
                                format!("table {table:?} requires input {name:?}"),
                            ));
                        }
                    }
                    for name in arguments.keys() {
                        if !required.contains(name) {
                            return Err(LootError::new(
                                entry.location.clone(),
                                format!("table {table:?} has no input {name:?}"),
                            ));
                        }
                    }
                }
                EntryKind::All(entries) | EntryKind::Choose { entries, .. } => {
                    self.validate_references(entries)?
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn check_cycles(
        &self,
        table: &Table,
        path: &mut Vec<String>,
        visited: &mut BTreeSet<String>,
    ) -> Result<(), LootError> {
        if visited.contains(&table.name) {
            return Ok(());
        }
        if path.contains(&table.name) {
            path.push(table.name.clone());
            return Err(LootError::new(
                table.location.clone(),
                format!("table reference cycle: {}", path.join(" -> ")),
            ));
        }
        path.push(table.name.clone());
        if path.len() > 256 {
            return Err(LootError::new(
                table.location.clone(),
                "table reference depth exceeds 256",
            ));
        }
        self.walk_references(&table.entries, path, visited)?;
        path.pop();
        visited.insert(table.name.clone());
        Ok(())
    }

    fn walk_references(
        &self,
        entries: &[Entry],
        path: &mut Vec<String>,
        visited: &mut BTreeSet<String>,
    ) -> Result<(), LootError> {
        for entry in entries {
            match &entry.kind {
                EntryKind::Table { table, .. } => {
                    self.check_cycles(&self.tables[table], path, visited)?
                }
                EntryKind::All(entries) | EntryKind::Choose { entries, .. } => {
                    self.walk_references(entries, path, visited)?
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn validate_expression_type(
    expression: &Expression,
    expected: &ValueType,
    location: &crate::SourceLocation,
) -> Result<(), LootError> {
    let valid = match expression {
        Expression::Literal(value) => expected.accepts(value),
        Expression::Range(min, max) => {
            expected.accepts(&Value::Integer(*min)) && expected.accepts(&Value::Integer(*max))
        }
        Expression::Not(_) => expected.accepts(&Value::Boolean(false)),
        Expression::Variable(_) => true,
    };
    if valid {
        Ok(())
    } else {
        Err(LootError::new(
            location.clone(),
            format!("expression expects {expected:?}"),
        ))
    }
}

fn validate_arguments(
    signature: &FunctionSignature,
    arguments: &BTreeMap<String, Expression>,
    require_all: bool,
    location: &crate::SourceLocation,
) -> Result<(), LootError> {
    for (name, expression) in arguments {
        let parameter = signature.parameters.get(name).ok_or_else(|| {
            LootError::new(location.clone(), format!("unknown argument {name:?}"))
        })?;
        validate_expression_type(expression, &parameter.value_type, location)?;
    }
    if require_all {
        for (name, parameter) in &signature.parameters {
            if !parameter.optional && !arguments.contains_key(name) {
                return Err(LootError::new(
                    location.clone(),
                    format!("missing argument {name:?}"),
                ));
            }
        }
    }
    Ok(())
}

fn lower_entries(
    entries: &mut [Entry],
    using: Option<&str>,
    defaults: &BTreeMap<String, Expression>,
    functions: &FunctionSignatures,
) -> Result<(), LootError> {
    for entry in entries {
        validate_expression_type(&entry.weight, &ValueType::Integer, &entry.location)?;
        if let Expression::Literal(Value::Integer(weight)) = &entry.weight
            && *weight < 0
        {
            return Err(LootError::new(
                entry.location.clone(),
                "weight cannot be negative",
            ));
        }
        if let Expression::Range(min, _) = &entry.weight
            && *min < 0
        {
            return Err(LootError::new(
                entry.location.clone(),
                "weight cannot be negative",
            ));
        }
        if let Some(condition) = &entry.condition {
            validate_expression_type(condition, &ValueType::Boolean, &entry.location)?;
        }
        if let EntryKind::Implicit { primary, arguments } = &entry.kind {
            entry.kind = if let Some(function) = using {
                let signature = functions.get(function).expect("default function validated");
                let mut combined = defaults.clone();
                combined.extend(arguments.clone());
                if let Some(primary) = primary {
                    let name = signature.primary.as_ref().ok_or_else(|| {
                        LootError::new(
                            entry.location.clone(),
                            "default function has no primary parameter; use named arguments",
                        )
                    })?;
                    if arguments.contains_key(name) {
                        return Err(LootError::new(
                            entry.location.clone(),
                            format!("primary parameter {name:?} was also supplied by name"),
                        ));
                    }
                    combined.insert(name.clone(), primary.clone());
                }
                EntryKind::Call {
                    function: function.into(),
                    arguments: combined,
                }
            } else {
                if !arguments.is_empty() {
                    return Err(LootError::new(
                        entry.location.clone(),
                        "named arguments require an explicit @function or using=function",
                    ));
                }
                let value = primary.clone().ok_or_else(|| {
                    LootError::new(
                        entry.location.clone(),
                        "entry needs a value or function call",
                    )
                })?;
                if matches!(value, Expression::Variable(_) | Expression::Not(_)) {
                    return Err(LootError::new(
                        entry.location.clone(),
                        "standalone variables are not returns; bindings already contribute to the generated bag",
                    ));
                }
                EntryKind::Value(value)
            };
        }
        match &mut entry.kind {
            EntryKind::Call {
                function,
                arguments,
            } => {
                let signature = functions.get(function).ok_or_else(|| {
                    LootError::new(
                        entry.location.clone(),
                        format!("unknown function {function:?}"),
                    )
                })?;
                validate_arguments(signature, arguments, true, &entry.location)?;
                if entry.binding.is_some() && signature.output.is_none() {
                    return Err(LootError::new(
                        entry.location.clone(),
                        "cannot bind a generator that emits no value",
                    ));
                }
            }
            EntryKind::All(children) => {
                lower_entries(Arc::make_mut(children), using, defaults, functions)?
            }
            EntryKind::Choose { entries, rolls, .. } => {
                validate_expression_type(rolls, &ValueType::Integer, &entry.location)?;
                if let Expression::Literal(Value::Integer(rolls)) = rolls
                    && *rolls < 0
                {
                    return Err(LootError::new(
                        entry.location.clone(),
                        "rolls cannot be negative",
                    ));
                }
                if let Expression::Range(min, _) = rolls
                    && *min < 0
                {
                    return Err(LootError::new(
                        entry.location.clone(),
                        "rolls cannot be negative",
                    ));
                }
                lower_entries(Arc::make_mut(entries), using, defaults, functions)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn collect_bindings(entries: &[Entry], names: &mut BTreeSet<String>) {
    for entry in entries {
        if let Some(name) = &entry.binding {
            names.insert(name.clone());
        }
        match &entry.kind {
            EntryKind::All(entries) | EntryKind::Choose { entries, .. } => {
                collect_bindings(entries, names)
            }
            _ => {}
        }
    }
}

fn infer_inputs(
    entries: &[Entry],
    locals: &mut BTreeSet<String>,
    declarations: &BTreeSet<String>,
    required: &mut BTreeSet<String>,
) -> Result<(), LootError> {
    for entry in entries {
        let mut expressions = vec![&entry.weight];
        expressions.extend(entry.condition.iter());
        match &entry.kind {
            EntryKind::Value(value) => expressions.push(value),
            EntryKind::Call { arguments, .. } | EntryKind::Table { arguments, .. } => {
                expressions.extend(arguments.values())
            }
            EntryKind::Choose { rolls, .. } => expressions.push(rolls),
            _ => {}
        }
        for expression in expressions {
            if let Some(name) = expression.variable()
                && !locals.contains(name)
            {
                if declarations.contains(name) {
                    return Err(LootError::new(
                        entry.location.clone(),
                        format!(
                            "binding ${name} is not available here (forward reference or escaped block scope)"
                        ),
                    ));
                }
                required.insert(name.into());
            }
        }
        match &entry.kind {
            EntryKind::All(entries) => {
                infer_inputs(entries, &mut locals.clone(), declarations, required)?
            }
            EntryKind::Choose { entries, .. } => {
                for child in entries.iter() {
                    infer_inputs(
                        std::slice::from_ref(child),
                        &mut locals.clone(),
                        declarations,
                        required,
                    )?;
                }
            }
            _ => {}
        }
        if let Some(name) = &entry.binding
            && (!locals.insert(name.clone()) || required.contains(name))
        {
            return Err(LootError::new(
                entry.location.clone(),
                format!("duplicate or shadowed binding {name:?}"),
            ));
        }
    }
    Ok(())
}
