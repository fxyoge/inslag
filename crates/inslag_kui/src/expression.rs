use crate::KuiValue as K;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

#[derive(Default)]
pub(crate) struct Evaluator {
    generation: u64,
    next_scope: usize,
    scopes: Vec<Scope>,
    programs: HashMap<String, Arc<Program>>,
}

struct Program {
    source: String,
    expression: Arc<inslag_luau::CompiledLuauExpression>,
}

#[derive(Default)]
struct Scope {
    // Repeated rows commonly have just one expression. Keep that entry inline
    // instead of allocating a hash table for every row during mounting.
    first: Option<Evaluation>,
    rest: HashMap<String, Evaluation>,
}

struct Evaluation {
    program: Arc<Program>,
    generation: u64,
    value: Option<Arc<K>>,
}

impl Evaluator {
    pub(crate) const UNCACHED_SCOPE: usize = usize::MAX;
    pub(crate) fn begin(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.scopes.clear();
        }
        self.next_scope = 0;
        self.enter(); // The mount's root lexical scope is always zero.
    }

    pub(crate) fn enter(&mut self) -> usize {
        let scope = self.next_scope;
        self.next_scope += 1;
        if scope == self.scopes.len() {
            self.scopes.push(Scope::default());
        }
        scope
    }

    pub(crate) fn finish(&mut self) {
        self.scopes.truncate(self.next_scope);
        // Results are shared only within this update. In particular, don't keep
        // a second owned collection alive throughout subsequent idle frames.
        for scope in &mut self.scopes {
            for entry in scope.first.iter_mut().chain(scope.rest.values_mut()) {
                entry.value = None;
            }
        }
    }

    pub(crate) fn evaluate(
        &mut self,
        scope: usize,
        source: &str,
        data: &BTreeMap<String, K>,
        aliases: &BTreeMap<String, K>,
    ) -> Option<K> {
        if scope == Self::UNCACHED_SCOPE {
            let program = program(&mut self.programs, source)?;
            return evaluate_compiled(source, &program.expression, data, aliases);
        }
        self.evaluate_shared(scope, source, data, aliases)
            .map(|value| (*value).clone())
    }

    pub(crate) fn evaluate_shared(
        &mut self,
        scope: usize,
        source: &str,
        data: &BTreeMap<String, K>,
        aliases: &BTreeMap<String, K>,
    ) -> Option<Arc<K>> {
        let cache = &mut self.scopes[scope];
        let first_matches = cache
            .first
            .as_ref()
            .is_some_and(|entry| entry.program.source == source);
        let entry = if first_matches {
            cache.first.as_mut().unwrap()
        } else if cache.first.is_none() || !cache.rest.contains_key(source) {
            let program = program(&mut self.programs, source)?.clone();
            let entry = Evaluation {
                program,
                generation: self.generation.wrapping_sub(1),
                value: None,
            };
            if cache.first.is_none() {
                cache.first.insert(entry)
            } else {
                cache.rest.entry(source.to_owned()).or_insert(entry)
            }
        } else {
            cache.rest.get_mut(source).unwrap()
        };
        if entry.generation != self.generation {
            entry.value =
                evaluate_compiled(source, &entry.program.expression, data, aliases).map(Arc::new);
            entry.generation = self.generation;
        }
        entry.value.clone()
    }
}

fn program<'a>(
    programs: &'a mut HashMap<String, Arc<Program>>,
    source: &str,
) -> Option<&'a Arc<Program>> {
    if !programs.contains_key(source) {
        programs.insert(
            source.to_owned(),
            Arc::new(Program {
                source: source.to_owned(),
                expression: environment().cached_expression(source, false).ok()?,
            }),
        );
    }
    programs.get(source)
}

pub(crate) fn roots(source: &str) -> Vec<String> {
    environment()
        .cached_expression(source, false)
        .expect("validated expression")
        .dependencies
        .clone()
}

fn evaluate_compiled(
    source: &str,
    expression: &inslag_luau::CompiledLuauExpression,
    data: &BTreeMap<String, K>,
    aliases: &BTreeMap<String, K>,
) -> Option<K> {
    let mut inputs = Vec::with_capacity(expression.dependencies.len());
    for root in &expression.dependencies {
        if let Some(value) = aliases.get(root) {
            inputs.push((root.as_str(), value));
        } else if let Some(value) = data.get(root) {
            inputs.push((root.as_str(), value));
        } else if !environment().is_builtin(root) {
            bevy::log::warn_once!("KUI expression `{source}` is missing input `{root}`");
            return None;
        }
    }
    match expression.evaluate_borrowed(&inputs) {
        Ok(value) => Some(value),
        Err(error) => {
            bevy::log::warn_once!("KUI expression `{source}`: {error}");
            None
        }
    }
}

/// The read-only Luau environment used by KUI bindings.
///
/// Use it to validate expressions with the same builtins available in a view.
///
/// ```
/// use inslag_kui::source::expression_environment;
///
/// let expression = expression_environment().compile("math.floor(price * quantity)", false)?;
/// assert_eq!(expression.dependencies, ["math", "price", "quantity"]);
/// # Ok::<(), inslag_luau::Error>(())
/// ```
pub fn environment() -> &'static inslag_luau::EnvironmentSpec {
    static SPEC: std::sync::OnceLock<inslag_luau::EnvironmentSpec> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| {
        inslag_luau::EnvironmentSpec::builder()
            .map()
            .globals([
                "assert", "error", "ipairs", "pairs", "next", "select", "tonumber", "tostring",
                "type", "typeof",
            ])
            .library("math", &["random", "randomseed"])
            .library("string", &[])
            .library("table", &[])
            .library("utf8", &[])
            .library("bit32", &[])
            .library("buffer", &[])
            .read_only(true)
            .build()
            .expect("valid KUI expression environment")
    })
}
