/// Find the sorted, unique global names referenced by a Luau expression.
///
/// Includes builtins and names in unexecuted branches. `item.price` contributes `item`.
///
/// ```
/// use inslag_kui::source::binding_roots;
///
/// assert_eq!(binding_roots("item.name .. suffix")?, ["item", "suffix"]);
/// # Ok::<(), &'static str>(())
/// ```
pub fn binding_roots(source: &str) -> Result<Vec<String>, &'static str> {
    crate::expression::environment()
        .cached_expression(source, false)
        .map(|expression| expression.dependencies.clone())
        .map_err(|_| "invalid Luau expression")
}

/// Owner/outer-alias inputs read by a subtree, accounting for component props
/// and caller-scoped slots. An unresolved caller crosses the subtree boundary;
/// those subtrees conservatively reconcile on every dirty update.
pub(crate) fn external_roots(node: &crate::KuiNode, aliases: &[String]) -> Option<Vec<String>> {
    use std::collections::BTreeSet;
    fn collect(
        node: &crate::KuiNode,
        aliases: &[String],
        callers: &[Vec<String>],
        result: &mut BTreeSet<String>,
    ) -> Option<()> {
        use crate::KuiNodeKind as N;
        let mut read = |source: &str| {
            for root in crate::expression::roots(source) {
                if !aliases.contains(&root) && !crate::expression::environment().is_builtin(&root) {
                    result.insert(root);
                }
            }
        };
        for path in node.bindings.iter().flat_map(crate::KuiBinding::paths) {
            read(path);
        }
        match &node.kind {
            N::Repeat {
                items,
                alias,
                template,
                ..
            } => {
                read(items);
                let mut nested = aliases.to_vec();
                nested.push(alias.clone());
                collect(template, &nested, callers, result)?;
            }
            N::Scope {
                inputs, template, ..
            } => {
                for input in inputs.values() {
                    if let crate::KuiPropSource::Binding(path) = input {
                        read(path);
                    }
                }
                let mut stack = callers.to_vec();
                stack.push(aliases.to_vec());
                collect(template, &["props".into()], &stack, result)?;
            }
            N::Caller { children, .. } => {
                let aliases = callers.last()?;
                for child in children {
                    collect(child, aliases, &callers[..callers.len() - 1], result)?;
                }
            }
            N::Node { children } | N::Button { children, .. } | N::Slot { children, .. } => {
                for child in children {
                    collect(child, aliases, callers, result)?;
                }
            }
            N::Meter { current, maximum } => {
                read(current);
                read(maximum);
            }
            _ => {}
        }
        Some(())
    }
    let mut result = BTreeSet::new();
    collect(node, aliases, &[], &mut result)?;
    Some(result.into_iter().collect())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BindingPath {
    pub(crate) root: String,
    pub(crate) segments: Vec<Segment>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Segment {
    Field(String),
    Position(usize),
    Key(Box<BindingPath>),
}

impl BindingPath {
    pub(crate) fn parse(source: &str) -> Result<Self, &'static str> {
        let mut parser = Parser {
            source: source.as_bytes(),
            offset: 0,
        };
        let path = parser.path(0)?;
        if parser.offset != parser.source.len() {
            return Err("path segments must be identifiers or bracket lookups");
        }
        Ok(path)
    }

    pub(crate) fn dependencies(&self) -> Vec<&Self> {
        let mut paths = vec![self];
        for segment in &self.segments {
            if let Segment::Key(key) = segment {
                paths.extend(key.dependencies());
            }
        }
        paths
    }
}

struct Parser<'a> {
    source: &'a [u8],
    offset: usize,
}

impl Parser<'_> {
    fn path(&mut self, depth: usize) -> Result<BindingPath, &'static str> {
        if depth > 32 {
            return Err("binding path nesting exceeds 32 levels");
        }
        let root = self.identifier()?;
        let mut segments = vec![];
        loop {
            match self.source.get(self.offset) {
                Some(b'.') => {
                    self.offset += 1;
                    segments.push(Segment::Field(self.identifier()?));
                }
                Some(b'[') => {
                    self.offset += 1;
                    let segment = if self.source.get(self.offset).is_some_and(u8::is_ascii_digit) {
                        Segment::Position(self.position()?)
                    } else {
                        Segment::Key(Box::new(self.path(depth + 1)?))
                    };
                    if self.source.get(self.offset) != Some(&b']') {
                        return Err("binding lookup requires a closing bracket");
                    }
                    self.offset += 1;
                    segments.push(segment);
                }
                _ => break,
            }
        }
        Ok(BindingPath { root, segments })
    }

    fn identifier(&mut self) -> Result<String, &'static str> {
        let start = self.offset;
        if !self
            .source
            .get(start)
            .is_some_and(|c| *c == b'_' || c.is_ascii_alphabetic())
        {
            return Err("path segments must be identifiers");
        }
        self.offset += 1;
        while self
            .source
            .get(self.offset)
            .is_some_and(|c| *c == b'_' || c.is_ascii_alphanumeric())
        {
            self.offset += 1;
        }
        Ok(String::from_utf8(self.source[start..self.offset].to_vec()).expect("ASCII identifier"))
    }

    fn position(&mut self) -> Result<usize, &'static str> {
        let start = self.offset;
        while self.source.get(self.offset).is_some_and(u8::is_ascii_digit) {
            self.offset += 1;
        }
        std::str::from_utf8(&self.source[start..self.offset])
            .unwrap()
            .parse()
            .map_err(|_| "sequence index is too large")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_and_dependencies_include_nested_key_paths() {
        assert_eq!(
            binding_roots("values[keys[row.id]].current").unwrap(),
            ["keys", "row", "values"]
        );
        assert_eq!(binding_roots("values[values.key]").unwrap(), ["values"]);
        let path = BindingPath::parse("values[keys[row.id]].current").unwrap();
        assert_eq!(
            path.dependencies()
                .iter()
                .map(|p| p.root.as_str())
                .collect::<Vec<_>>(),
            ["values", "keys", "row"]
        );
    }

    #[test]
    fn computed_or_malformed_accesses_are_left_to_luau() {
        for path in [
            "values[]",
            "values[row.id",
            "values[row.id + 1]",
            "values[f()]",
            "values[-1]",
            "values[true]extra",
            "values..id",
        ] {
            assert!(BindingPath::parse(path).is_err(), "{path}");
        }
    }
}
