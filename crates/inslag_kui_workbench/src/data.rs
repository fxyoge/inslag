use std::collections::BTreeMap;

use inslag_kui::source as kui;
use inslag_state_tree::LuauValue as KuiValue;
use kui::{KuiBinding, KuiDefinition, KuiNode, KuiNodeKind, KuiTextFormat, KuiTextPart};

pub(crate) type Data = BTreeMap<String, KuiValue>;

pub(crate) fn load(root: &std::path::Path, path: &str) -> Result<KuiDefinition, String> {
    fn collect(
        root: &std::path::Path,
        path: &str,
        sources: &mut BTreeMap<String, String>,
    ) -> Result<(), String> {
        if sources.contains_key(path) {
            return Ok(());
        }
        let source =
            std::fs::read_to_string(root.join(path)).map_err(|error| format!("{path}: {error}"))?;
        let definition = kui::parse(path, &source).map_err(|error| error.to_string())?;
        sources.insert(path.into(), source);
        for import in definition.imports.values() {
            collect(root, import, sources)?;
        }
        Ok(())
    }
    let mut sources = BTreeMap::new();
    collect(root, path, &mut sources)?;
    kui::compile(path, sources).map_err(|error| error.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Lookup,
    Sequence,
    Record,
    Text,
    String,
    Integer,
    Number,
    Boolean,
    Scalar,
    Conflict,
}

#[derive(Clone, Debug)]
pub(crate) struct Binding {
    pub path: String,
    pub kind: Kind,
    pub uses: Vec<String>,
    pub sample: KuiValue,
}

pub(crate) fn bindings(definition: &KuiDefinition) -> Vec<Binding> {
    fn visit(
        nodes: &[KuiNode],
        result: &mut BTreeMap<String, Binding>,
        aliases: &[String],
        callers: &[Vec<String>],
    ) {
        for node in nodes {
            if let KuiNodeKind::Scope { props, inputs, .. } = &node.kind {
                for prop in props {
                    if let Some(kui::KuiPropSource::Binding(path)) = inputs.get(&prop.name) {
                        let (kind, sample) = prop_sample(prop);
                        record(
                            result,
                            aliases,
                            node,
                            path,
                            kind,
                            format!("prop {}", prop.name),
                            sample,
                        );
                    }
                }
            }
            if let KuiNodeKind::Meter { current, maximum } = &node.kind {
                record(
                    result,
                    aliases,
                    node,
                    current,
                    Kind::Number,
                    "meter current".into(),
                    KuiValue::Integer(0),
                );
                record(
                    result,
                    aliases,
                    node,
                    maximum,
                    Kind::Number,
                    "meter maximum".into(),
                    KuiValue::Integer(1),
                );
            }
            if let KuiNodeKind::Repeat {
                items,
                alias,
                template,
                ..
            } = &node.kind
            {
                record(
                    result,
                    aliases,
                    node,
                    items,
                    Kind::Sequence,
                    "collection".into(),
                    KuiValue::Sequence(vec![]),
                );
                let mut nested = aliases.to_vec();
                nested.push(alias.clone());
                visit(std::slice::from_ref(template), result, &nested, callers);
                continue;
            }
            for binding in &node.bindings {
                if let KuiBinding::TextTemplate(parts) = binding {
                    for part in parts {
                        if let KuiTextPart::Binding { path, format } = part {
                            let kind = if *format == KuiTextFormat::Plain {
                                Kind::Text
                            } else {
                                Kind::Number
                            };
                            record(
                                result,
                                aliases,
                                node,
                                path,
                                kind,
                                "template".into(),
                                KuiValue::Integer(0),
                            );
                        }
                    }
                    continue;
                }
                let (path, kind, operation, sample) = match binding {
                    KuiBinding::Property { name, path }
                        if matches!(name.as_str(), "position" | "scale") =>
                    {
                        (
                            path,
                            Kind::Record,
                            name.clone(),
                            KuiValue::Record(if name == "position" {
                                [
                                    ("x".into(), KuiValue::Integer(0)),
                                    ("y".into(), KuiValue::Integer(0)),
                                    ("z".into(), KuiValue::Integer(0)),
                                ]
                                .into()
                            } else {
                                [
                                    ("x".into(), KuiValue::Integer(1)),
                                    ("y".into(), KuiValue::Integer(1)),
                                ]
                                .into()
                            }),
                        )
                    }
                    KuiBinding::Property { name, path } => (
                        path,
                        Kind::Number,
                        name.clone(),
                        KuiValue::Integer(if name.starts_with("scale-") { 1 } else { 0 }),
                    ),
                    KuiBinding::Value(path) => (path, Kind::Scalar, "value".into(), KuiValue::Nil),
                    KuiBinding::Text(path) => (
                        path,
                        Kind::Text,
                        "text".to_owned(),
                        KuiValue::String(match &node.kind {
                            KuiNodeKind::Text { value } => value.clone(),
                            _ => String::new(),
                        }),
                    ),
                    KuiBinding::FormattedText { path, format } => (
                        path,
                        if *format == KuiTextFormat::Plain {
                            Kind::Text
                        } else {
                            Kind::Number
                        },
                        "text".into(),
                        KuiValue::Integer(0),
                    ),
                    KuiBinding::TextTemplate(_) => unreachable!(),
                    KuiBinding::Visible(path) => (
                        path,
                        Kind::Boolean,
                        "visible".into(),
                        KuiValue::Boolean(true),
                    ),
                    KuiBinding::Disabled(path) => (
                        path,
                        Kind::Boolean,
                        "disabled".into(),
                        KuiValue::Boolean(false),
                    ),
                    KuiBinding::Attribute { name, expression } => (
                        expression,
                        Kind::Scalar,
                        format!("attr-{name}"),
                        KuiValue::Nil,
                    ),
                };
                record(result, aliases, node, path, kind, operation, sample);
            }
            if let KuiNodeKind::Node { children } | KuiNodeKind::Button { children, .. } =
                &node.kind
            {
                visit(children, result, aliases, callers);
            }
            match &node.kind {
                KuiNodeKind::Scope { template, .. } => {
                    let mut stack = callers.to_vec();
                    stack.push(aliases.to_vec());
                    visit(
                        std::slice::from_ref(template),
                        result,
                        &["props".into()],
                        &stack,
                    );
                }
                KuiNodeKind::Caller { children, .. } => {
                    let caller = callers.last().expect("compiled caller");
                    visit(children, result, caller, &callers[..callers.len() - 1]);
                }
                KuiNodeKind::Slot { children, .. } => visit(children, result, aliases, callers),
                _ => {}
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(&definition.children, &mut result, &[], &[]);
    if let Some(contract) = &definition.component {
        for prop in &contract.props {
            let (kind, sample) = prop_sample(prop);
            let path = format!("props.{}", prop.name);
            result.insert(
                path.clone(),
                Binding {
                    path,
                    kind,
                    sample,
                    uses: vec![format!("component prop {}", prop.name)],
                },
            );
        }
    }
    result.into_values().collect()
}

fn prop_sample(prop: &kui::KuiProp) -> (Kind, KuiValue) {
    use kui::KuiPropType as T;
    let (kind, fallback) = match prop.kind {
        T::Boolean => (Kind::Boolean, KuiValue::Boolean(false)),
        T::Integer | T::Number => (
            if prop.kind == T::Integer {
                Kind::Integer
            } else {
                Kind::Number
            },
            KuiValue::Integer(if prop.name == "maximum" { 1 } else { 0 }),
        ),
        T::String => (Kind::String, KuiValue::String(String::new())),
        T::Record => (Kind::Record, KuiValue::Record(BTreeMap::new())),
        T::Sequence => (Kind::Sequence, KuiValue::Sequence(vec![])),
        T::Scalar => (Kind::Scalar, KuiValue::Nil),
    };
    (
        kind,
        prop.default
            .as_ref()
            .and_then(|value| prop.kind.literal(value).ok())
            .unwrap_or(fallback),
    )
}

fn record(
    result: &mut BTreeMap<String, Binding>,
    aliases: &[String],
    node: &KuiNode,
    path: &str,
    kind: Kind,
    operation: String,
    sample: KuiValue,
) {
    let roots = kui::binding_roots(path).unwrap_or_default();
    if roots.is_empty()
        || roots
            .iter()
            .all(|root| kui::expression_environment().is_builtin(root))
    {
        return;
    }
    // Computed expressions expose backing inputs rather than inventing fields
    // named after their source. Direct fields retain inferred editor types.
    if path.contains([
        ' ', '(', '{', '+', '-', '*', '/', '=', '<', '>', '\'', '"', ':',
    ]) {
        for root in roots {
            if kui::expression_environment().is_builtin(&root) {
                continue;
            }
            record(
                result,
                aliases,
                node,
                &root,
                Kind::Scalar,
                operation.clone(),
                KuiValue::Nil,
            );
        }
        return;
    }
    // A lookup's leaf depends on runtime keys (possibly repeat aliases). Expose its
    // backing map for raw editing instead of inventing a literal "map[key]" field.
    if path.contains('[') {
        let root = path
            .split(['.', '['])
            .next()
            .expect("validated binding path");
        if !aliases.iter().any(|alias| alias == root) {
            record(
                result,
                aliases,
                node,
                root,
                Kind::Lookup,
                operation,
                KuiValue::Record(BTreeMap::new()),
            );
        }
        return;
    }
    if aliases
        .iter()
        .any(|alias| path.split('.').next() == Some(alias.as_str()))
    {
        return;
    }
    let usage = format!("{}: {operation}", node.id.as_deref().unwrap_or("(unnamed)"));
    if let Some(existing) = result.get_mut(path) {
        if (existing.kind == Kind::Lookup && matches!(kind, Kind::Record | Kind::Sequence))
            || existing.kind == Kind::Scalar
            || (existing.kind == Kind::Text
                && matches!(
                    kind,
                    Kind::Boolean | Kind::Number | Kind::Integer | Kind::String
                ))
            || (existing.kind == Kind::Number && kind == Kind::Integer)
        {
            existing.kind = kind;
            existing.sample = sample;
        } else if kind != Kind::Scalar
            && !(kind == Kind::Lookup && matches!(existing.kind, Kind::Record | Kind::Sequence))
            && kind != existing.kind
            && !(kind == Kind::Text
                && matches!(
                    existing.kind,
                    Kind::Boolean | Kind::Number | Kind::Integer | Kind::String
                ))
            && !(kind == Kind::Number && existing.kind == Kind::Integer)
        {
            existing.kind = Kind::Conflict;
        }
        existing.uses.push(usage);
    } else {
        result.insert(
            path.into(),
            Binding {
                path: path.into(),
                kind,
                uses: vec![usage],
                sample,
            },
        );
    }
}

pub(crate) fn get<'a>(data: &'a Data, path: &str) -> Option<&'a KuiValue> {
    let mut parts = path.split('.');
    let mut value = data.get(parts.next()?)?;
    for part in parts {
        let KuiValue::Record(record) = value else {
            return None;
        };
        value = record.get(part)?;
    }
    Some(value)
}

/// Refuse to silently replace a scalar ancestor of a dotted path.
pub(crate) fn set(data: &mut Data, path: &str, value: Option<KuiValue>) -> Result<(), String> {
    fn descend(data: &mut Data, parts: &[&str], value: Option<KuiValue>) -> Result<(), String> {
        if parts.len() == 1 {
            if let Some(value) = value {
                data.insert(parts[0].into(), value);
            } else {
                data.remove(parts[0]);
            }
            return Ok(());
        }
        if !data.contains_key(parts[0]) && value.is_none() {
            return Ok(());
        }
        let entry = data
            .entry(parts[0].into())
            .or_insert_with(|| KuiValue::Record(Data::new()));
        let KuiValue::Record(record) = entry else {
            return Err(format!(
                "{} is not a record; edit its parent in the raw map",
                parts[0]
            ));
        };
        descend(record, &parts[1..], value)
    }
    descend(data, &path.split('.').collect::<Vec<_>>(), value)
}

pub(crate) fn encode(data: &Data) -> String {
    serde_json::to_string_pretty(&to_json(&KuiValue::Record(data.clone())))
        .expect("KUI data is serializable")
}

pub(crate) fn to_json(value: &KuiValue) -> serde_json::Value {
    use serde_json::{Value, json};
    match value {
        KuiValue::Nil => Value::Null,
        KuiValue::Boolean(value) => json!(value),
        KuiValue::String(value) => json!(value),
        KuiValue::Integer(value) => json!(value),
        KuiValue::Number(value) => json!(value),
        KuiValue::Sequence(values) => Value::Array(values.iter().map(to_json).collect()),
        KuiValue::Record(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), to_json(value)))
                .collect(),
        ),
    }
}

pub(crate) fn decode(text: &str) -> Result<Data, String> {
    let value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    match from_json(value)? {
        KuiValue::Record(data) => Ok(data),
        _ => Err("the backing data must be a JSON object".into()),
    }
}

pub(crate) fn from_json(value: serde_json::Value) -> Result<KuiValue, String> {
    use serde_json::Value;
    Ok(match value {
        Value::Null => KuiValue::Nil,
        Value::Bool(value) => KuiValue::Boolean(value),
        Value::String(value) => KuiValue::String(value),
        Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                KuiValue::Integer(integer)
            } else if value.is_f64() {
                KuiValue::Number(value.as_f64().ok_or("invalid number")?)
            } else {
                return Err("integer is outside the signed 64-bit range".into());
            }
        }
        Value::Array(values) => KuiValue::Sequence(
            values
                .into_iter()
                .map(from_json)
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(values) => KuiValue::Record(
            values
                .into_iter()
                .map(|(key, value)| Ok((key, from_json(value)?)))
                .collect::<Result<_, String>>()?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use inslag_kui::source as inslag_kui;
    #[test]
    fn expressions_keep_builtins_out_of_inputs_and_compound_targets_get_record_samples() {
        let definition = inslag_kui::parse("expressions.kui", r#"<view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="test"><node bind:position="pose" bind:scale="scale"><text bind:text="utf8.len(title)"/><text bind:text="math.pi"/></node></view>"#).unwrap();
        let fields = super::bindings(&definition);
        assert_eq!(
            fields
                .iter()
                .map(|field| field.path.as_str())
                .collect::<Vec<_>>(),
            ["pose", "scale", "title"]
        );
        assert!(matches!(&fields[0].sample, KuiValue::Record(record) if record.contains_key("z")));
        assert!(
            matches!(&fields[1].sample, KuiValue::Record(record) if record.get("x") == Some(&KuiValue::Integer(1)))
        );
    }
    #[test]
    fn lookups_expose_backing_tables_without_fabricating_indexed_fields() {
        let definition = inslag_kui::parse("synthetic", r#"<view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="test"><node><repeat bind:items="rows" as="row" key="id"><text template="{health[row.id].current}"/></repeat><text bind:text="items[index]"/></node></view>"#).unwrap();
        let bindings = super::bindings(&definition);
        assert_eq!(
            bindings
                .iter()
                .map(|b| (&*b.path, &b.kind))
                .collect::<Vec<_>>(),
            [
                ("health", &super::Kind::Lookup),
                ("items", &super::Kind::Lookup),
                ("rows", &super::Kind::Sequence)
            ]
        );
    }
    use super::*;

    #[test]
    fn source_bundle_compiler_discovers_imported_typed_props_without_live_runtime_assets() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("panel.kui"), r#"<view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="panel"><import name="choice" src="choice.kui"/><node><use component="choice"><prop name="enabled" bind:value="enabled"/></use></node></view>"#).unwrap();
        std::fs::write(root.path().join("choice.kui"), r#"<component xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="choice"><prop name="enabled" type="boolean"/><button bind:disabled="not props.enabled"/></component>"#).unwrap();
        let definition = load(root.path(), "panel.kui").unwrap();
        let controls = bindings(&definition);
        assert_eq!(controls.len(), 1);
        assert_eq!(controls[0].path, "enabled");
        assert_eq!(controls[0].kind, Kind::Boolean);
    }

    #[test]
    fn component_contracts_and_scoped_slots_discover_typed_caller_controls() {
        let mut component = inslag_kui::parse("synthetic", r#"<component xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="range"><prop name="current" type="number" default="42"/><prop name="maximum" type="number" default="100"/><prop name="title" type="string" default="Health"/><node><text bind:text="props.title"/><meter bind:current="props.current" bind:maximum="props.maximum"/><slot name="detail"/></node></component>"#).unwrap();
        let direct = bindings(&component);
        assert_eq!(
            direct
                .iter()
                .map(|b| (b.path.as_str(), &b.kind))
                .collect::<Vec<_>>(),
            [
                ("props.current", &Kind::Number),
                ("props.maximum", &Kind::Number),
                ("props.title", &Kind::String)
            ]
        );
        assert_eq!(direct[0].sample, KuiValue::Integer(42));
        let mut view = inslag_kui::parse("synthetic", r#"<view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="test"><text bind:text="owner.detail"/></view>"#).unwrap();
        let slot = view.children.remove(0);
        let KuiNodeKind::Node { children } = &mut component.children[0].kind else {
            panic!()
        };
        children[2].kind = KuiNodeKind::Caller {
            children: vec![slot],
            style: None,
        };
        view.children.push(KuiNode {
            attributes: Default::default(),
            id: None,
            class: None,
            bindings: vec![],
            kind: KuiNodeKind::Scope {
                name: "synthetic.kui".into(),
                props: component.component.unwrap().props,
                inputs: BTreeMap::from([(
                    "current".into(),
                    inslag_kui::KuiPropSource::Binding("owner.health".into()),
                )]),
                template: Box::new(component.children.remove(0)),
                style: None,
            },
        });
        let result = bindings(&view);
        assert_eq!(
            result
                .iter()
                .map(|b| (b.path.as_str(), &b.kind))
                .collect::<Vec<_>>(),
            [
                ("owner.detail", &Kind::Text),
                ("owner.health", &Kind::Number)
            ]
        );
    }

    #[test]
    fn templates_discover_owner_paths_and_unify_numeric_and_boolean_text() {
        let definition = inslag_kui::parse("synthetic", r#"<view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="test"><node><text template="{amount:grouped} / {limit} · {enabled}"/><node bind:visible="enabled"/><repeat bind:items="items" as="item" key="id"><text template="{item.name} · {amount:fixed.2}"/></repeat></node></view>"#).unwrap();
        let result = bindings(&definition);
        assert_eq!(
            result
                .iter()
                .map(|binding| (binding.path.as_str(), &binding.kind))
                .collect::<Vec<_>>(),
            vec![
                ("amount", &Kind::Number),
                ("enabled", &Kind::Boolean),
                ("items", &Kind::Sequence),
                ("limit", &Kind::Text)
            ]
        );
        assert_eq!(result[0].uses.len(), 2);
    }

    #[test]
    fn collection_controls_discover_owner_dependencies_without_local_alias_fields() {
        let definition = inslag_kui::parse("synthetic", r#"<view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="test"><node><repeat bind:items="groups" as="group" key="id"><node><repeat bind:items="group.rows" as="row" key="id"><button on-click="select" bind:value="row.id"><text bind:text="group.name"/><text bind:text="row.name"/><text bind:text="treasury"/></button></repeat></node></repeat></node></view>"#).unwrap();
        let result = bindings(&definition);
        assert_eq!(
            result
                .iter()
                .map(|binding| binding.path.as_str())
                .collect::<Vec<_>>(),
            ["groups", "treasury"]
        );
        assert_eq!(result[0].sample, KuiValue::Sequence(vec![]));
        assert_eq!(result[0].kind, Kind::Sequence);
    }

    #[test]
    fn nested_edits_preserve_siblings_and_reject_scalar_ancestors() {
        let mut data = Data::new();
        set(
            &mut data,
            "ui.title",
            Some(KuiValue::String("Hello".into())),
        )
        .unwrap();
        set(&mut data, "ui.open", Some(KuiValue::Boolean(true))).unwrap();
        set(&mut data, "ui.title", None).unwrap();
        assert_eq!(get(&data, "ui.open"), Some(&KuiValue::Boolean(true)));
        assert_eq!(get(&data, "ui.title"), None);
        let before = data.clone();
        assert!(set(&mut data, "ui.open.child", Some(KuiValue::Nil)).is_err());
        assert_eq!(data, before);
    }

    #[test]
    fn raw_map_preserves_all_types_and_absence() {
        let data = decode(r#"{"nil":null,"bool":true,"int":1,"number":1.0,"nested":{"text":"hello"},"list":[false,2]}"#).unwrap();
        assert_eq!(get(&data, "int"), Some(&KuiValue::Integer(1)));
        assert_eq!(get(&data, "number"), Some(&KuiValue::Number(1.0)));
        assert_eq!(get(&data, "nil"), Some(&KuiValue::Nil));
        assert_eq!(get(&data, "missing"), None);
        assert_eq!(decode(&encode(&data)).unwrap(), data);
        assert!(decode(r#"{"overflow":18446744073709551615}"#).is_err());
        assert!(decode("[]").is_err());
    }

    #[test]
    fn discovery_unifies_boolean_attribute_uses_and_reports_conflicts() {
        let definition = inslag_kui::parse("synthetic.kui", r#"<view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="test"><node><button on-click="test" bind:attr-disabled="locked" bind:disabled="locked"><text bind:text="title">Fallback</text></button><text bind:text="conflict" format="grouped"/><node bind:visible="conflict"/></node></view>"#).unwrap();
        let result = bindings(&definition);
        let locked = result
            .iter()
            .find(|binding| binding.path == "locked")
            .unwrap();
        assert_eq!(locked.kind, Kind::Boolean);
        assert_eq!(locked.uses.len(), 2);
        assert_eq!(locked.sample, KuiValue::Boolean(false));
        assert_eq!(
            result
                .iter()
                .find(|binding| binding.path == "title")
                .unwrap()
                .sample,
            KuiValue::String("Fallback".into())
        );
        assert_eq!(
            result
                .iter()
                .find(|binding| binding.path == "conflict")
                .unwrap()
                .kind,
            Kind::Conflict
        );
    }
}
