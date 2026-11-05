use crate::program::{Entry, EntryKind, Expression};
use crate::{Arguments, Catalog, LootError, SourceLocation, Value, ValueType};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub max_steps: usize,
    pub max_depth: usize,
    pub max_results: usize,
    pub max_calls: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_steps: 100_000,
            max_depth: 64,
            max_results: 10_000,
            max_calls: 10_000,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FunctionCall {
    pub function: String,
    pub arguments: Arguments,
    pub location: SourceLocation,
    pub seed: u64,
    pub index: usize,
    pub(crate) signature: crate::FunctionSignature,
}

#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) enum TraceEvent {
    Selected {
        location: SourceLocation,
        child: usize,
    },
    Called {
        location: SourceLocation,
        function: String,
        seed: u64,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum Step {
    Call(FunctionCall),
    Waiting,
    Finished(Vec<Value>),
}

#[derive(Debug)]
struct Frame {
    entries: Arc<[Entry]>,
    position: usize,
    locals: BTreeMap<String, Value>,
    values: Vec<Value>,
    binding: Option<String>,
    location: SourceLocation,
    table: Option<String>,
}

#[derive(Debug)]
struct Pending {
    binding: Option<String>,
    location: SourceLocation,
    expected: Option<ValueType>,
}

#[derive(Debug)]
pub(crate) struct Session {
    catalog: Arc<Catalog>,
    random: fastrand::Rng,
    frames: Vec<Frame>,
    pending: Option<Pending>,
    finished: Option<Vec<Value>>,
    failure: Option<LootError>,
    limits: Limits,
    steps: usize,
    calls: usize,
    #[cfg(test)]
    trace: Vec<TraceEvent>,
}

impl Session {
    pub(crate) fn new(
        catalog: Arc<Catalog>,
        table: &str,
        inputs: BTreeMap<String, Value>,
        seed: u64,
    ) -> Result<Self, LootError> {
        let table = catalog
            .tables
            .get(table)
            .ok_or_else(|| LootError::request(format!("unknown table {table:?}")))?;
        validate_inputs(&catalog, &table.name, &inputs, &table.location)?;
        let frame = Frame {
            entries: table.entries.clone(),
            position: 0,
            locals: inputs,
            values: Vec::new(),
            binding: None,
            location: table.location.clone(),
            table: Some(table.name.clone()),
        };
        Ok(Self {
            catalog,
            random: fastrand::Rng::with_seed(seed),
            frames: vec![frame],
            pending: None,
            finished: None,
            failure: None,
            limits: Limits::default(),
            steps: 0,
            calls: 0,
            #[cfg(test)]
            trace: Vec::new(),
        })
    }

    pub(crate) fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }
    #[cfg(test)]
    pub(crate) fn trace(&self) -> &[TraceEvent] {
        &self.trace
    }

    pub(crate) fn advance(&mut self) -> Result<Step, LootError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = self.advance_inner().map_err(|mut error| {
            if error.table_path.is_empty() {
                error.table_path = self
                    .frames
                    .iter()
                    .filter_map(|frame| frame.table.clone())
                    .collect();
            }
            error
        });
        if let Err(error) = &result {
            self.failure = Some(error.clone());
        }
        result
    }

    fn advance_inner(&mut self) -> Result<Step, LootError> {
        if self.pending.is_some() {
            return Ok(Step::Waiting);
        }
        if let Some(values) = &self.finished {
            return Ok(Step::Finished(values.clone()));
        }
        loop {
            let frame = self
                .frames
                .last_mut()
                .expect("unfinished session has a frame");
            if frame.position == frame.entries.len() {
                let frame = self.frames.pop().expect("frame exists");
                if self.frames.is_empty() {
                    let generated = frame.values;
                    self.finished = Some(generated.clone());
                    return Ok(Step::Finished(generated));
                }
                self.accept(frame.values, frame.binding, &frame.location)?;
                continue;
            }
            let entry = frame.entries[frame.position].clone();
            frame.position += 1;
            self.steps += 1;
            if self.steps > self.limits.max_steps {
                return Err(self.error(&entry.location, "evaluation step limit exceeded"));
            }
            let locals = &self.frames.last().expect("frame exists").locals;
            if !condition(&entry, locals, &mut self.random)? {
                if entry.binding.is_some() {
                    return Err(self.error(&entry.location, "cannot bind a skipped entry"));
                }
                continue;
            }
            match &entry.kind {
                EntryKind::Nothing => self.accept(Vec::new(), entry.binding, &entry.location)?,
                EntryKind::Value(expression) => {
                    let value = evaluate(expression, locals, &mut self.random, &entry.location)?;
                    self.accept(vec![value], entry.binding, &entry.location)?;
                }
                EntryKind::Call {
                    function,
                    arguments,
                } => {
                    self.calls += 1;
                    if self.calls > self.limits.max_calls {
                        return Err(self.error(&entry.location, "function call limit exceeded"));
                    }
                    let values =
                        evaluate_arguments(arguments, locals, &mut self.random, &entry.location)?;
                    let signature = self
                        .catalog
                        .functions
                        .get(function)
                        .expect("linked function exists");
                    let arguments = signature.arguments(values, &entry.location)?;
                    let seed = self.random.u64(..);
                    self.pending = Some(Pending {
                        binding: entry.binding,
                        location: entry.location.clone(),
                        expected: signature.output.clone(),
                    });
                    #[cfg(test)]
                    self.trace.push(TraceEvent::Called {
                        location: entry.location.clone(),
                        function: function.clone(),
                        seed,
                    });
                    return Ok(Step::Call(FunctionCall {
                        function: function.clone(),
                        arguments,
                        location: entry.location,
                        seed,
                        index: self.calls,
                        signature: signature.clone(),
                    }));
                }
                EntryKind::Table { table, arguments } => {
                    let inputs =
                        evaluate_arguments(arguments, locals, &mut self.random, &entry.location)?;
                    let table = &self.catalog.tables[table];
                    validate_inputs(&self.catalog, &table.name, &inputs, &entry.location)?;
                    let frame = Frame {
                        entries: table.entries.clone(),
                        position: 0,
                        locals: inputs,
                        values: Vec::new(),
                        binding: entry.binding,
                        location: entry.location.clone(),
                        table: Some(table.name.clone()),
                    };
                    self.push(frame)?;
                }
                EntryKind::All(entries) => {
                    let frame = Frame {
                        entries: entries.clone(),
                        position: 0,
                        locals: locals.clone(),
                        values: Vec::new(),
                        binding: entry.binding,
                        location: entry.location.clone(),
                        table: None,
                    };
                    self.push(frame)?;
                }
                EntryKind::Choose {
                    entries,
                    rolls,
                    distinct,
                } => {
                    let count = integer(
                        evaluate(rolls, locals, &mut self.random, &entry.location)?,
                        "rolls",
                        &entry.location,
                    )?;
                    if count > self.limits.max_steps as u64 {
                        return Err(self.error(&entry.location, "roll count exceeds step limit"));
                    }
                    let mut candidates = Vec::new();
                    for (index, child) in entries.iter().enumerate() {
                        if condition(child, locals, &mut self.random)? {
                            let weight = integer(
                                evaluate(&child.weight, locals, &mut self.random, &child.location)?,
                                "weight",
                                &child.location,
                            )?;
                            if weight > 0 {
                                candidates.push((index, weight));
                            }
                        }
                    }
                    let mut selected = Vec::new();
                    for _ in 0..count {
                        if candidates.is_empty() {
                            if selected.is_empty() {
                                break;
                            }
                            return Err(self.error(
                                &entry.location,
                                "distinct choice exhausted its eligible entries",
                            ));
                        }
                        let total = candidates
                            .iter()
                            .try_fold(0u64, |sum, (_, weight)| sum.checked_add(*weight))
                            .ok_or_else(|| {
                                self.error(&entry.location, "choice weights overflow u64")
                            })?;
                        let mut roll = self.random.u64(0..total);
                        let position = candidates
                            .iter()
                            .position(|(_, weight)| {
                                if roll < *weight {
                                    true
                                } else {
                                    roll -= *weight;
                                    false
                                }
                            })
                            .expect("weighted roll selects candidate");
                        let index = candidates[position].0;
                        let mut selected_entry = entries[index].clone();
                        selected_entry.condition = None;
                        selected.push(Entry {
                            location: selected_entry.location.clone(),
                            kind: EntryKind::All(vec![selected_entry].into()),
                            binding: None,
                            condition: None,
                            weight: Expression::Literal(Value::Integer(1)),
                        });
                        #[cfg(test)]
                        self.trace.push(TraceEvent::Selected {
                            location: entry.location.clone(),
                            child: index,
                        });
                        if *distinct {
                            candidates.remove(position);
                        }
                    }
                    let frame = Frame {
                        entries: selected.into(),
                        position: 0,
                        locals: locals.clone(),
                        values: Vec::new(),
                        binding: entry.binding,
                        location: entry.location.clone(),
                        table: None,
                    };
                    self.push(frame)?;
                }
            }
        }
    }

    pub(crate) fn resume(&mut self, result: Result<Vec<Value>, String>) -> Result<(), LootError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let pending = self
            .pending
            .take()
            .ok_or_else(|| LootError::request("session is not awaiting a function result"))?;
        let result = (|| {
            let generated = result.map_err(|message| {
                self.error(&pending.location, format!("function failed: {message}"))
            })?;
            for output in generated.iter() {
                if !pending
                    .expected
                    .as_ref()
                    .is_some_and(|expected| expected.accepts(output))
                {
                    return Err(self.error(
                        &pending.location,
                        format!(
                            "generator emitted {output:?}; expected {:?}",
                            pending.expected
                        ),
                    ));
                }
            }
            self.accept(
                generated.into_iter().collect(),
                pending.binding,
                &pending.location,
            )
        })();
        if let Err(error) = &result {
            self.failure = Some(error.clone());
        }
        result
    }

    fn push(&mut self, frame: Frame) -> Result<(), LootError> {
        if self.frames.len() >= self.limits.max_depth {
            return Err(self.error(&frame.location, "evaluation depth limit exceeded"));
        }
        self.frames.push(frame);
        Ok(())
    }

    fn accept(
        &mut self,
        values: Vec<Value>,
        binding: Option<String>,
        location: &SourceLocation,
    ) -> Result<(), LootError> {
        if binding.is_some() && values.len() != 1 {
            return Err(self.error(
                location,
                format!("binding requires exactly one result, got {}", values.len()),
            ));
        }
        let frame = self.frames.last_mut().expect("parent frame exists");
        if let Some(binding) = binding {
            if frame.locals.contains_key(&binding) {
                return Err(self.error(location, format!("binding {binding:?} already exists")));
            }
            frame.locals.insert(binding, values[0].clone());
        }
        if frame.values.len().saturating_add(values.len()) > self.limits.max_results {
            return Err(self.error(location, "result limit exceeded"));
        }
        frame.values.extend(values);
        Ok(())
    }

    fn error(&self, location: &SourceLocation, message: impl Into<String>) -> LootError {
        let mut error = LootError::new(location.clone(), message);
        error.table_path = self
            .frames
            .iter()
            .filter_map(|frame| frame.table.clone())
            .collect();
        error
    }
}

fn condition(
    entry: &Entry,
    locals: &BTreeMap<String, Value>,
    random: &mut fastrand::Rng,
) -> Result<bool, LootError> {
    match &entry.condition {
        None => Ok(true),
        Some(expression) => match evaluate(expression, locals, random, &entry.location)? {
            Value::Boolean(value) => Ok(value),
            _ => Err(LootError::new(
                entry.location.clone(),
                "when requires a boolean",
            )),
        },
    }
}

fn integer(value: Value, name: &str, location: &SourceLocation) -> Result<u64, LootError> {
    match value {
        Value::Integer(value) if value >= 0 => Ok(value as u64),
        _ => Err(LootError::new(
            location.clone(),
            format!("{name} requires a nonnegative integer"),
        )),
    }
}

fn evaluate_arguments(
    arguments: &BTreeMap<String, Expression>,
    locals: &BTreeMap<String, Value>,
    random: &mut fastrand::Rng,
    location: &SourceLocation,
) -> Result<BTreeMap<String, Value>, LootError> {
    arguments
        .iter()
        .map(|(name, expression)| {
            Ok((
                name.clone(),
                evaluate(expression, locals, random, location)?,
            ))
        })
        .collect()
}

fn evaluate(
    expression: &Expression,
    locals: &BTreeMap<String, Value>,
    random: &mut fastrand::Rng,
    location: &SourceLocation,
) -> Result<Value, LootError> {
    match expression {
        Expression::Literal(value) => Ok(value.clone()),
        Expression::Range(min, max) => Ok(Value::Integer(random.i64(*min..=*max))),
        Expression::Variable(name) => locals
            .get(name)
            .cloned()
            .ok_or_else(|| LootError::new(location.clone(), format!("missing variable ${name}"))),
        Expression::Not(name) => match locals.get(name) {
            Some(Value::Boolean(value)) => Ok(Value::Boolean(!value)),
            _ => Err(LootError::new(
                location.clone(),
                format!("!${name} requires a boolean"),
            )),
        },
    }
}

fn validate_inputs(
    catalog: &Catalog,
    table: &str,
    inputs: &BTreeMap<String, Value>,
    location: &SourceLocation,
) -> Result<(), LootError> {
    let required = &catalog.inputs[table];
    for name in required {
        if !inputs.contains_key(name) {
            return Err(LootError::new(
                location.clone(),
                format!("table {table:?} requires input {name:?}"),
            ));
        }
    }
    for name in inputs.keys() {
        if !required.contains(name) {
            return Err(LootError::new(
                location.clone(),
                format!("table {table:?} has no input {name:?}"),
            ));
        }
    }
    if inputs.values().any(|value| !value.is_valid()) {
        return Err(LootError::new(
            location.clone(),
            "context values must contain only finite numbers",
        ));
    }
    Ok(())
}
