//! Compile Luau expressions once, then evaluate them against Rust values.
//!
//! Configure the available builtins with [`EnvironmentSpec`], compile an
//! expression, and pass inputs as [`LuauValue`]s. Simple reads use native fast
//! paths; other expressions run in Luau automatically. You don't need to manage
//! a VM, and compiled expressions can be shared across threads.
//!
//! # Compile and evaluate
//!
//! ```
//! use inslag_luau::{EnvironmentSpec, LuauValue as V};
//! use std::collections::BTreeMap;
//!
//! let environment = EnvironmentSpec::builder()
//!     .library("math", &["random", "randomseed"])
//!     .read_only(true)
//!     .build()?;
//! let expression = environment.compile("math.floor(base * multiplier)", false)?;
//! let mut inputs = BTreeMap::from([
//!     ("base".into(), V::Integer(10)),
//!     ("multiplier".into(), V::Number(1.25)),
//! ]);
//!
//! assert_eq!(expression.evaluate(&inputs)?, V::Integer(12));
//! inputs.insert("base".into(), V::Integer(20));
//! assert_eq!(expression.evaluate(&inputs)?, V::Integer(25));
//! # Ok::<(), inslag_luau::Error>(())
//! ```
//!
//! Supply a single expression, without `return`. The second argument to
//! [`compile`](EnvironmentSpec::compile) chooses the result: `false` keeps the
//! value; `true` converts it to a boolean using Luau truthiness. Only `nil` and
//! `false` are falsy; zero and empty strings are truthy.
//!
//! Keep the compiled expression for repeated evaluation. If several callers
//! request the same source, use [`cached_expression`](EnvironmentSpec::cached_expression)
//! on the same environment (or a clone) to reuse compilation.
//!
//! # Choose what expressions can access
//!
//! No standard globals are exposed by default. Add only what you need:
//!
//! - [`.globals(["type", "tostring"])`](EnvironmentBuilder::globals) exposes named builtins.
//! - [`.library("math", &["random", "randomseed"])`](EnvironmentBuilder::library)
//!   exposes a library **except** the listed entries. An empty exclusion list
//!   exposes the whole library.
//! - [`.read_only(true)`](EnvironmentBuilder::read_only) prevents changes to input
//!   tables and assignments to globals. This is off by default. Local variables
//!   and tables created by the expression can still be changed.
//! - [`.map()`](EnvironmentBuilder::map) adds the sequence helper shown below.
//!
//! Input names become globals, so `item.price` reads the `price` field of an
//! `item` record. Missing names and fields read as `nil`. Inputs cannot shadow
//! exposed builtins, even when the input value is `Nil`.
//!
//! Compilation checks syntax, not whether names or library functions exist.
//! Evaluation can still fail: for example, calling an unavailable function,
//! indexing `nil`, exceeding execution limits, or returning an unsupported value.
//! [`prepare_current_thread`](EnvironmentSpec::prepare_current_thread) can check
//! builtin configuration before the first evaluation.
//!
//! # Transform and filter sequences
//!
//! Opt into `map(sequence, callback)` with [`.map()`](EnvironmentBuilder::map).
//! It visits elements in order and omits `nil` results. `false` and zero are kept.
//!
//! ```
//! use inslag_luau::{EnvironmentSpec, LuauValue as V};
//!
//! let environment = EnvironmentSpec::builder().map().read_only(true).build()?;
//! let expression = environment.compile(
//!     "map(values, function(v) return if v > 0 then v * 2 else nil end)",
//!     false,
//! )?;
//! let values = V::Sequence(vec![V::Integer(-1), V::Integer(3), V::Integer(5)]);
//!
//! assert_eq!(
//!     expression.evaluate_borrowed(&[("values", &values)])?,
//!     V::Sequence(vec![V::Integer(6), V::Integer(10)]),
//! );
//! # Ok::<(), inslag_luau::Error>(())
//! ```
//!
//! # Numeric formulas
//!
//! Use [`numeric::NumericExpression`] when inputs are numeric slots and the result
//! must be a number. It evaluates without allocations or VM calls. Unlike the
//! general expression API, it rejects unsupported syntax rather than falling
//! back to Luau.
//!
//! ```
//! use inslag_luau::numeric::NumericExpression;
//!
//! let expression = NumericExpression::compile("base * multiplier", &["base", "multiplier"])?;
//! assert_eq!(expression.evaluate(&[10.0, 1.5])?, 15.0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Saving values and expressions
//!
//! Enable the `serde` feature to serialize [`LuauValue`], [`EnvironmentSpec`],
//! [`CompiledLuauExpression`], and [`numeric::NumericExpression`]. Expressions
//! save their source and configuration, not VM bytecode, and recompile when
//! deserialized. Deserialization can therefore fail if compilation fails.
mod environment;
mod native;
pub mod numeric;
#[cfg(feature = "serde")]
mod persistence;

pub use environment::{EnvironmentBuilder, EnvironmentSpec};
pub use mlua::Error;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap, HashSet},
    rc::Rc,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use full_moon::visitors::Visitor;
use mlua::{ChunkMode, Compiler, Function, Lua, Table, Value};

const FUNCTION_CACHE_CAPACITY: usize = 256;

static NEXT_EXPRESSION_ID: AtomicU64 = AtomicU64::new(1);

/// An owned input or result, with no references to a Luau VM.
///
/// Sequences use 1-based indexing in expressions; records use string keys.
/// A `Nil` table entry is absent, not a stored value. Empty tables evaluate to
/// `Sequence(vec![])`, including empty records passed in as inputs.
///
/// Numbers follow Luau's double-precision semantics, including `Integer` inputs.
/// Large integers can lose precision, and an integral `Number` can return as an
/// `Integer`. Do not rely on preserving the input's numeric variant through
/// evaluation.
///
/// Returned tables must be dense sequences or string-keyed records. Mixed keys,
/// holes, shared or cyclic tables, and non-value results such as functions are
/// rejected. Strings must be UTF-8; returned values are also subject to size and
/// nesting limits.
#[derive(Clone, Debug, PartialEq)]
pub enum LuauValue {
    /// Luau `nil`, also used for missing names and fields.
    Nil,
    /// Luau `true` or `false`.
    Boolean(bool),
    /// An integer, subject to Luau's numeric precision during evaluation.
    Integer(i64),
    /// A floating-point number.
    Number(f64),
    /// A UTF-8 string.
    String(String),
    /// A sequence indexed from 1 in Luau, not 0.
    Sequence(Vec<LuauValue>),
    /// String-keyed fields, accessible as `value.field` or `value["field"]`.
    Record(BTreeMap<String, LuauValue>),
}

/// An expression ready to evaluate repeatedly with different inputs.
///
/// Create one with [`EnvironmentSpec::compile`] or
/// [`EnvironmentSpec::cached_expression`]. It retains its environment, so the
/// original specification does not need to stay in scope. Clones and shared
/// references can be used across threads.
#[derive(Clone, Debug)]
pub struct CompiledLuauExpression {
    #[cfg(feature = "serde")]
    source: Box<str>,
    #[cfg(feature = "serde")]
    boolean: bool,
    identity: u64,
    bytecode: Box<[u8]>,
    /// Sorted, deduplicated global names referenced by the expression.
    ///
    /// Use these to decide which input changes require reevaluation. Names in
    /// unexecuted branches and nested functions are included, as are builtins
    /// such as `math`; fields such as `item.price` contribute only `item`.
    /// Treat this list as read-only: evaluation also uses it to select inputs.
    pub dependencies: Vec<String>,
    native: Option<native::Plan>,
    environment: EnvironmentSpec,
}

impl CompiledLuauExpression {
    /// Whether this expression has a native fast path.
    ///
    /// This does not guarantee VM-free evaluation: unresolved roots, for example,
    /// can require a fallback. Normally, just call [`evaluate`](Self::evaluate).
    pub fn is_native(&self) -> bool {
        self.native.is_some()
    }

    /// Evaluate with named inputs and return an owned value.
    ///
    /// Input keys become globals alongside the configured builtins. Inputs are
    /// not retained between calls, and evaluating never changes the Rust values
    /// passed in. The result must fit [`LuauValue`]'s supported value shapes.
    ///
    /// Returns an error for builtin name collisions, Luau execution failures,
    /// or a result that cannot be converted to [`LuauValue`].
    pub fn evaluate(&self, inputs: &BTreeMap<String, LuauValue>) -> Result<LuauValue, Error> {
        evaluate_inputs(
            self,
            inputs.iter().map(|(name, value)| (name.as_str(), value)),
            std::iter::empty(),
        )
    }

    /// Evaluate borrowed name/value pairs instead of building an input map.
    ///
    /// Names must be unique. This avoids cloning Rust records and sequences to
    /// assemble inputs; VM evaluation still converts them to Luau tables. The
    /// returned value is owned and does not borrow from the inputs.
    ///
    /// ```
    /// use inslag_luau::{EnvironmentSpec, LuauValue as V};
    ///
    /// let environment = EnvironmentSpec::builder().build()?;
    /// let expression = environment.compile("item.price", false)?;
    /// let item = V::Record([("price".into(), V::Integer(12))].into());
    /// assert_eq!(expression.evaluate_borrowed(&[("item", &item)])?, V::Integer(12));
    /// # Ok::<(), inslag_luau::Error>(())
    /// ```
    pub fn evaluate_borrowed(&self, inputs: &[(&str, &LuauValue)]) -> Result<LuauValue, Error> {
        evaluate_inputs(self, inputs.iter().copied(), std::iter::empty())
    }

    /// Evaluate in Luau even when a native fast path is available.
    ///
    /// Use this to compare results or benchmark against
    /// [`evaluate`](Self::evaluate). Input and result rules are the same.
    pub fn evaluate_vm(&self, inputs: &BTreeMap<String, LuauValue>) -> Result<LuauValue, Error> {
        for name in inputs.keys() {
            self.environment.validate_input_name(name)?;
        }
        WORKER.with_borrow_mut(|worker| {
            worker.evaluate(
                self,
                inputs.iter().map(|(name, value)| (name.as_str(), value)),
                std::iter::empty(),
            )
        })
    }
}

impl EnvironmentSpec {
    /// Compile or reuse an expression with this source and boolean mode.
    ///
    /// Calls on this environment and its clones share a cache. Separately built
    /// environments do not, even if configured identically. Entries can be
    /// evicted; keep the returned [`Arc`] if you need to reuse an expression.
    /// See [`compile`](Self::compile) for source and boolean-mode rules.
    pub fn cached_expression(
        &self,
        source: &str,
        boolean: bool,
    ) -> Result<Arc<CompiledLuauExpression>, Error> {
        type Key = (u64, String, bool);
        static CACHE: OnceLock<Mutex<HashMap<Key, Arc<CompiledLuauExpression>>>> = OnceLock::new();
        let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
        let key = (self.identity(), source.to_owned(), boolean);
        if let Some(expression) = cache.get(&key) {
            return Ok(expression.clone());
        }
        let expression = Arc::new(self.compile(source, boolean)?);
        if cache.len() >= 1024 {
            cache.clear();
        }
        cache.insert(key, expression.clone());
        Ok(expression)
    }

    /// Compile a single Luau expression in this environment.
    ///
    /// Pass expression text such as `item.price * 2`, without a `return` statement.
    /// Set `boolean` to `true` for a condition: the result is coerced to
    /// [`LuauValue::Boolean`] using Luau truthiness, where only `nil` and `false`
    /// are false. With `boolean = false`, evaluation returns the expression's value.
    ///
    /// Returns an error for invalid syntax. Names and builtin configuration are
    /// resolved during evaluation, not compilation.
    ///
    /// ```
    /// use inslag_luau::{EnvironmentSpec, LuauValue as V};
    ///
    /// let environment = EnvironmentSpec::builder().build()?;
    /// let condition = environment.compile("remaining", true)?;
    /// assert_eq!(
    ///     condition.evaluate_borrowed(&[("remaining", &V::Integer(0))])?,
    ///     V::Boolean(true),
    /// );
    /// # Ok::<(), inslag_luau::Error>(())
    /// ```
    pub fn compile(
        &self,
        expression: &str,
        boolean: bool,
    ) -> Result<CompiledLuauExpression, Error> {
        let source = if boolean {
            format!("return not not ({expression})")
        } else {
            format!("return ({expression})")
        };
        let bytecode = Compiler::new().compile(&source)?;
        let ast = full_moon::parse(&source).map_err(|e| Error::runtime(format!("{e:?}")))?;
        let mut dependencies = native::Dependencies::default();
        dependencies.visit_ast(&ast);
        Ok(CompiledLuauExpression {
            #[cfg(feature = "serde")]
            source: expression.into(),
            #[cfg(feature = "serde")]
            boolean,
            identity: NEXT_EXPRESSION_ID.fetch_add(1, Ordering::Relaxed),
            bytecode: bytecode.into_boxed_slice(),
            dependencies: dependencies.roots.into_iter().collect(),
            native: native::compile(&ast),
            environment: self.clone(),
        })
    }
    /// Check and initialize the configured builtins on the calling thread.
    ///
    /// Optional: evaluation does this automatically. Call it during worker setup
    /// to move initialization out of the first evaluation, or to catch unknown
    /// builtins and invalid library names early. It does not execute expressions
    /// or validate their inputs.
    pub fn prepare_current_thread(&self) -> Result<(), Error> {
        WORKER.with_borrow_mut(|worker| worker.prepare(self).map(|_| ()))
    }
}

thread_local! {
    static WORKER: RefCell<LuauWorker> = RefCell::new(LuauWorker::new());
}

/// Evaluate parallel input-name/value slices with extra contextual bindings.
///
/// Use this when input names are stored separately from their values. For an
/// ordinary map or borrowed pairs, use [`CompiledLuauExpression::evaluate`] or
/// [`CompiledLuauExpression::evaluate_borrowed`] instead.
///
/// `names` and `values` must have equal lengths, with unique names. Extra bindings
/// override ordinary inputs with the same name; names within `extra` must also
/// be unique. Contextual tables are read-only regardless of the environment's
/// input policy. No binding can shadow an exposed builtin.
///
/// Pass `None` for no context, `Some((name, &value))` for one binding, or an array
/// of pairs for several. Returns the same evaluation errors as
/// [`CompiledLuauExpression::evaluate`], plus an error for mismatched slice lengths.
///
/// ```
/// use inslag_luau::{EnvironmentSpec, LuauValue as V, evaluate};
///
/// let environment = EnvironmentSpec::builder().read_only(true).build()?;
/// let expression = environment.compile("base * context.multiplier", false)?;
/// let names = [Box::<str>::from("base")];
/// let values = [V::Integer(10)];
/// let context = V::Record([("multiplier".into(), V::Integer(3))].into());
///
/// assert_eq!(
///     evaluate(&expression, &names, &values, Some(("context", &context)))?,
///     V::Integer(30),
/// );
/// # Ok::<(), inslag_luau::Error>(())
/// ```
pub fn evaluate<'a, C>(
    expression: &CompiledLuauExpression,
    names: &'a [Box<str>],
    values: &'a [LuauValue],
    extra: C,
) -> Result<LuauValue, Error>
where
    C: IntoIterator<Item = (&'a str, &'a LuauValue)>,
    C::IntoIter: Clone,
{
    if names.len() != values.len() {
        return Err(Error::runtime(
            "expression input names and values must have equal lengths",
        ));
    }
    evaluate_inputs(
        expression,
        names
            .iter()
            .zip(values)
            .map(|(name, value)| (name.as_ref(), value)),
        extra.into_iter(),
    )
}

fn evaluate_inputs<'a>(
    expression: &CompiledLuauExpression,
    inputs: impl Iterator<Item = (&'a str, &'a LuauValue)> + Clone,
    extra: impl Iterator<Item = (&'a str, &'a LuauValue)> + Clone,
) -> Result<LuauValue, Error> {
    for (name, _) in inputs.clone() {
        expression.environment.validate_input_name(name)?;
    }
    for (name, _) in extra.clone() {
        expression.environment.validate_input_name(name)?;
    }
    let lookup = |root: &str| {
        extra
            .clone()
            .find(|(name, _)| *name == root)
            .map(|(_, value)| value)
            .or_else(|| {
                inputs
                    .clone()
                    .find_map(|(name, value)| (name == root).then_some(value))
            })
    };
    if let Some(plan) = &expression.native
        // Undeclared roots may address builtins and must use the VM.
        && plan.roots_declared(&lookup)
    {
        return native::normalize(plan.evaluate(&lookup)?);
    }
    WORKER.with_borrow_mut(|worker| worker.evaluate(expression, inputs, extra))
}

struct LuauWorker {
    lua: Lua,
    interrupts: Rc<Cell<usize>>,
    functions: HashMap<u64, CachedFunction>,
    environments: HashMap<u64, Table>,
    cache_access: u64,
}

struct CachedFunction {
    function: Function,
    environment: Table,
    last_access: u64,
}

impl LuauWorker {
    fn new() -> Self {
        let lua = Lua::new();
        lua.sandbox(true)
            .expect("Luau sandbox initialization failed");
        lua.set_memory_limit(64 * 1024 * 1024)
            .expect("Luau memory limit initialization failed");
        let interrupts = Rc::new(Cell::new(0));
        let callback_interrupts = Rc::clone(&interrupts);
        lua.set_interrupt(move |_| {
            let count = callback_interrupts.get() + 1;
            callback_interrupts.set(count);
            if count > 10_000 {
                return Err(Error::runtime("Luau execution limit exceeded"));
            }
            Ok(mlua::VmState::Continue)
        });
        Self {
            lua,
            interrupts,
            functions: HashMap::new(),
            environments: HashMap::new(),
            cache_access: 0,
        }
    }

    fn evaluate<'a>(
        &mut self,
        expression: &CompiledLuauExpression,
        inputs: impl Iterator<Item = (&'a str, &'a LuauValue)>,
        extra: impl IntoIterator<Item = (&'a str, &'a LuauValue)>,
    ) -> Result<LuauValue, Error> {
        self.interrupts.set(0);
        let (function, environment) = self.function(expression)?;
        environment.set_readonly(false);
        environment.clear()?;

        for (name, value) in inputs {
            if !expression.environment.read_only()
                || expression.dependencies.iter().any(|root| root == name)
            {
                let value = to_lua(&self.lua, value)?;
                if expression.environment.read_only() {
                    freeze(&value)?;
                }
                environment.raw_set(name, value)?;
            }
        }
        for (name, value) in extra {
            let value = to_lua(&self.lua, value)?;
            freeze(&value)?;
            environment.raw_set(name, value)?;
        }

        if expression.environment.read_only() {
            environment.set_readonly(true);
        }
        let value = function.call(())?;
        from_lua(value, &mut Conversion::new(), 0)
    }

    fn function(
        &mut self,
        expression: &CompiledLuauExpression,
    ) -> Result<(Function, Table), Error> {
        self.cache_access = self.cache_access.wrapping_add(1);
        if let Some(cached) = self.functions.get_mut(&expression.identity) {
            cached.last_access = self.cache_access;
            return Ok((cached.function.clone(), cached.environment.clone()));
        }
        self.functions.remove(&expression.identity);

        if self.functions.len() == FUNCTION_CACHE_CAPACITY {
            let oldest = self
                .functions
                .iter()
                .min_by_key(|(_, cached)| cached.last_access)
                .map(|(identity, _)| *identity)
                .expect("a full function cache is not empty");
            self.functions.remove(&oldest);
        }
        let function = self
            .lua
            .load(expression.bytecode.as_ref())
            .set_mode(ChunkMode::Binary)
            .into_function()?;
        let metatable = self.prepare(&expression.environment)?;
        let environment = self.lua.create_table()?;
        environment.set_metatable(Some(metatable))?;
        function.set_environment(environment.clone())?;
        self.functions.insert(
            expression.identity,
            CachedFunction {
                function: function.clone(),
                environment: environment.clone(),
                last_access: self.cache_access,
            },
        );
        Ok((function, environment))
    }

    fn prepare(&mut self, spec: &EnvironmentSpec) -> Result<Table, Error> {
        if let Some(metatable) = self.environments.get(&spec.identity()) {
            return Ok(metatable.clone());
        }
        let metatable = spec.materialize(&self.lua)?;
        self.environments.insert(spec.identity(), metatable.clone());
        Ok(metatable)
    }
}

fn freeze(value: &Value) -> Result<(), Error> {
    if let Value::Table(table) = value {
        for pair in table.pairs::<Value, Value>() {
            freeze(&pair?.1)?;
        }
        table.set_readonly(true);
    }
    Ok(())
}

// Strings have Luau's standard string-library __index behavior. Delegate this
// uncommon read to the VM rather than approximating its metatable in Rust.
fn string_index(key: &LuauValue) -> Result<native::Evaluated<'static>, Error> {
    WORKER.with_borrow(|worker| {
        let library: Table = worker.lua.globals().get("string")?;
        let value = library.raw_get::<Value>(to_lua(&worker.lua, key)?)?;
        if matches!(value, Value::Function(_)) {
            Ok(native::Evaluated::Function)
        } else {
            from_lua(value, &mut Conversion::new(), 0)
                .map(|v| native::Evaluated::Portable(std::borrow::Cow::Owned(v)))
        }
    })
}

fn to_lua(lua: &Lua, value: &LuauValue) -> Result<Value, Error> {
    Ok(match value {
        LuauValue::Nil => Value::Nil,
        LuauValue::Boolean(value) => Value::Boolean(*value),
        LuauValue::Integer(value) => Value::Integer(*value),
        LuauValue::Number(value) => Value::Number(*value),
        LuauValue::String(value) => Value::String(lua.create_string(value.as_bytes())?),
        LuauValue::Sequence(values) => {
            let table = lua.create_table_with_capacity(values.len(), 0)?;
            for (index, value) in values.iter().enumerate() {
                table.set(index + 1, to_lua(lua, value)?)?;
            }
            Value::Table(table)
        }
        LuauValue::Record(values) => {
            let table = lua.create_table_with_capacity(0, values.len())?;
            for (key, value) in values {
                table.set(key.as_str(), to_lua(lua, value)?)?;
            }
            Value::Table(table)
        }
    })
}

struct Conversion {
    seen_tables: HashSet<usize>,
    remaining_values: usize,
}

impl Conversion {
    fn new() -> Self {
        Self {
            seen_tables: HashSet::new(),
            remaining_values: 100_000,
        }
    }

    fn consume(&mut self) -> Result<(), Error> {
        self.remaining_values = self
            .remaining_values
            .checked_sub(1)
            .ok_or_else(|| Error::runtime("Luau value exceeds the persistence size limit"))?;
        Ok(())
    }
}

fn from_lua(value: Value, conversion: &mut Conversion, depth: usize) -> Result<LuauValue, Error> {
    conversion.consume()?;
    if depth == 64 {
        return Err(Error::runtime("Luau value nesting exceeds 64 levels"));
    }
    Ok(match value {
        Value::Nil => LuauValue::Nil,
        Value::Boolean(value) => LuauValue::Boolean(value),
        Value::Integer(value) => LuauValue::Integer(value),
        Value::Number(value) => LuauValue::Number(value),
        Value::String(value) => {
            let value = value.to_str()?;
            LuauValue::String(value.to_string())
        }
        Value::Table(table) => from_table(table, conversion, depth + 1)?,
        other => {
            return Err(Error::runtime(format!(
                "Luau value type `{}` cannot be persisted",
                other.type_name()
            )));
        }
    })
}

fn from_table(table: Table, conversion: &mut Conversion, depth: usize) -> Result<LuauValue, Error> {
    if !conversion.seen_tables.insert(table.to_pointer() as usize) {
        return Err(Error::runtime(
            "shared or cyclic Luau tables cannot be persisted",
        ));
    }
    let mut sequence = BTreeMap::new();
    let mut record = BTreeMap::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        match key {
            Value::Integer(index) if index > 0 => {
                sequence.insert(index as usize, from_lua(value, conversion, depth)?);
            }
            Value::String(key) => {
                let key = key.to_str()?;
                record.insert(key.to_string(), from_lua(value, conversion, depth)?);
            }
            _ => {
                return Err(Error::runtime(
                    "Luau tables require string or positive integer keys",
                ));
            }
        }
    }
    if !sequence.is_empty() && !record.is_empty() {
        return Err(Error::runtime(
            "mixed sequence and record Luau tables cannot be persisted",
        ));
    }
    if !record.is_empty() {
        return Ok(LuauValue::Record(record));
    }
    let expected: Vec<_> = (1..=sequence.len()).collect();
    if sequence.keys().copied().collect::<Vec<_>>() != expected {
        return Err(Error::runtime("sparse Luau sequences cannot be persisted"));
    }
    Ok(LuauValue::Sequence(sequence.into_values().collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_namespace_survives_function_eviction_without_sharing_input_tables() {
        let spec = EnvironmentSpec::builder()
            .library("math", &["random", "randomseed"])
            .read_only(true)
            .build()
            .unwrap();
        let mut worker = LuauWorker::new();
        let shared = worker.prepare(&spec).unwrap();
        let first = spec.compile("math.floor(value)", false).unwrap();
        assert_eq!(
            worker
                .evaluate(
                    &first,
                    [("value", &LuauValue::Number(1.5))].into_iter(),
                    None
                )
                .unwrap(),
            LuauValue::Integer(1)
        );
        let first_environment = worker.functions[&first.identity].environment.clone();
        for index in 0..FUNCTION_CACHE_CAPACITY {
            let other = spec
                .compile(&format!("math.floor({index} + 0.5)"), false)
                .unwrap();
            worker.evaluate(&other, std::iter::empty(), None).unwrap();
            let environment = &worker.functions[&other.identity].environment;
            assert_ne!(environment.to_pointer(), first_environment.to_pointer());
            assert_eq!(
                environment.metatable().unwrap().to_pointer(),
                shared.to_pointer()
            );
        }
        assert!(!worker.functions.contains_key(&first.identity));
        assert_eq!(
            worker
                .evaluate(
                    &first,
                    [("value", &LuauValue::Number(2.5))].into_iter(),
                    None
                )
                .unwrap(),
            LuauValue::Integer(2)
        );
        assert_eq!(worker.functions.len(), FUNCTION_CACHE_CAPACITY);
        assert_eq!(worker.environments.len(), 1);
        assert_eq!(
            worker.prepare(&spec).unwrap().to_pointer(),
            shared.to_pointer()
        );
    }

    #[test]
    fn specifications_isolate_compilation_and_vm_namespaces() {
        let allowed = EnvironmentSpec::builder()
            .library("math", &[])
            .build()
            .unwrap();
        let omitted = EnvironmentSpec::builder()
            .library("math", &["floor"])
            .build()
            .unwrap();
        let a = allowed.cached_expression("math.floor(1.5)", false).unwrap();
        let b = omitted.cached_expression("math.floor(1.5)", false).unwrap();
        assert_ne!(a.identity, b.identity);
        assert_eq!(
            a.identity,
            allowed
                .clone()
                .cached_expression("math.floor(1.5)", false)
                .unwrap()
                .identity
        );
        assert_eq!(a.evaluate(&BTreeMap::new()).unwrap(), LuauValue::Integer(1));
        assert!(b.evaluate(&BTreeMap::new()).is_err());
        assert!(
            allowed.cached_expression("true", true).unwrap().identity
                != allowed.cached_expression("true", false).unwrap().identity
        );
        let empty = EnvironmentSpec::builder().build().unwrap();
        assert_eq!(
            empty
                .compile("math", false)
                .unwrap()
                .evaluate(&BTreeMap::new())
                .unwrap(),
            LuauValue::Nil
        );
    }

    fn compile_expression(source: &str, boolean: bool) -> Result<CompiledLuauExpression, Error> {
        static SPEC: OnceLock<EnvironmentSpec> = OnceLock::new();
        SPEC.get_or_init(|| {
            EnvironmentSpec::builder()
                .globals(["string", "math"])
                .build()
                .unwrap()
        })
        .compile(source, boolean)
    }

    #[test]
    fn persists_portable_sequences_and_records() {
        let expression =
            compile_expression("{ values = { 1, 2, 3 }, enabled = true }", false).unwrap();

        let value = evaluate(&expression, &[], &[], None).unwrap();

        assert_eq!(
            value,
            LuauValue::Record(BTreeMap::from([
                ("enabled".into(), LuauValue::Boolean(true)),
                (
                    "values".into(),
                    LuauValue::Sequence(vec![
                        LuauValue::Integer(1),
                        LuauValue::Integer(2),
                        LuauValue::Integer(3),
                    ]),
                ),
            ]))
        );
    }

    #[test]
    fn evaluates_standard_string_functions() {
        let expression = compile_expression("string.upper('ready')", false).unwrap();

        assert_eq!(
            evaluate(&expression, &[], &[], None).unwrap(),
            LuauValue::String("READY".into())
        );
    }

    #[test]
    fn evaluation_environments_do_not_share_authored_globals() {
        let mutate = compile_expression(
            "(function() accidental_global = 42 return accidental_global end)()",
            false,
        )
        .unwrap();
        let inspect = compile_expression("accidental_global", false).unwrap();

        assert_eq!(
            evaluate(&mutate, &[], &[], None).unwrap(),
            LuauValue::Integer(42)
        );
        assert_eq!(evaluate(&inspect, &[], &[], None).unwrap(), LuauValue::Nil);
    }

    #[test]
    fn cached_evaluation_environments_are_cleared() {
        let expression = compile_expression(
            "(function() local result = accidental_global or (context and context.name); accidental_global = 42 return result end)()",
            false,
        )
        .unwrap();

        let context = LuauValue::Record(BTreeMap::from([(
            "name".into(),
            LuauValue::String("tick".into()),
        )]));
        assert_eq!(
            evaluate(&expression, &[], &[], Some(("context", &context))).unwrap(),
            LuauValue::String("tick".into())
        );
        assert_eq!(
            evaluate(&expression, &[], &[], None).unwrap(),
            LuauValue::Nil
        );
    }

    #[test]
    fn contextual_inputs_cannot_shadow_builtins() {
        let expression = compile_expression("math", false).unwrap();

        assert!(evaluate(&expression, &[], &[], Some(("math", &LuauValue::Nil))).is_err());
        assert!(evaluate(&expression, &[Box::from("math")], &[LuauValue::Nil], None).is_err());
    }

    #[test]
    fn interrupts_unbounded_expressions() {
        let expression =
            compile_expression("(function() while true do end return true end)()", false).unwrap();

        let error = evaluate(&expression, &[], &[], None).unwrap_err();

        assert!(error.to_string().contains("Luau execution limit exceeded"));
    }

    #[test]
    fn rejects_shared_table_graphs_without_expanding_them() {
        let expression = compile_expression(
            "(function() local shared = { value = 1 } return { a = shared, b = shared } end)()",
            false,
        )
        .unwrap();

        let error = evaluate(&expression, &[], &[], None).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("shared or cyclic Luau tables cannot be persisted")
        );
    }
}
