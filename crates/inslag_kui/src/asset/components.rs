use super::*;
use std::{collections::BTreeMap, future::Future, pin::Pin};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KuiComponentContract {
    pub props: Vec<KuiProp>,
    pub events: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KuiProp {
    pub name: String,
    pub kind: KuiPropType,
    pub default: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KuiPropType {
    String,
    Boolean,
    Integer,
    Number,
    Record,
    Sequence,
    Scalar,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KuiPropSource {
    Literal(String),
    Binding(String),
}

impl KuiPropType {
    pub fn accepts(&self, value: &crate::KuiValue) -> bool {
        use crate::KuiValue as V;
        match (self, value) {
            (Self::String, V::String(_))
            | (Self::Boolean, V::Boolean(_))
            | (Self::Integer, V::Integer(_))
            | (Self::Number, V::Integer(_))
            | (Self::Record, V::Record(_))
            | (Self::Sequence, V::Sequence(_)) => true,
            (Self::Number, V::Number(n)) => n.is_finite(),
            (Self::Scalar, V::Nil | V::String(_) | V::Boolean(_) | V::Integer(_)) => true,
            (Self::Scalar, V::Number(n)) => n.is_finite(),
            _ => false,
        }
    }

    pub fn literal(&self, text: &str) -> Result<crate::KuiValue, String> {
        use crate::KuiValue as V;
        let value = match self {
            Self::String | Self::Scalar => Some(V::String(text.into())),
            Self::Boolean => text.parse().ok().map(V::Boolean),
            Self::Integer => text.parse().ok().map(V::Integer),
            Self::Number => text.parse::<i64>().ok().map(V::Integer).or_else(|| {
                text.parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .map(V::Number)
            }),
            Self::Record | Self::Sequence => None,
        };
        value.ok_or_else(|| format!("`{text}` is not a valid {self:?} literal"))
    }
}

pub(super) fn declaration(
    reader: &XmlReader<&[u8]>,
    source: &str,
    start: &BytesStart<'_>,
    tag: &str,
    root: &str,
    imports: &mut BTreeMap<String, String>,
    contract: &mut KuiComponentContract,
) -> Result<bool, KuiParseError> {
    if !matches!(tag, "import" | "prop" | "event") {
        return Ok(false);
    }
    let attrs = attributes(reader, source, start)?;
    let fail = |message| error(reader, source, message);
    let get = |name: &str| {
        attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    let name = get("name").ok_or_else(|| fail(format!("<{tag}> requires name")))?;
    if tag == "prop" {
        identifier(&name)
    } else {
        label(&name)
    }
    .map_err(fail)?;
    let allowed: &[&str] = match tag {
        "import" => &["name", "src"],
        "prop" => &["name", "type", "default"],
        _ => &["name"],
    };
    if attrs
        .iter()
        .any(|(key, _)| !allowed.contains(&key.as_str()))
    {
        return Err(fail(format!("unknown <{tag}> attribute")));
    }
    match tag {
        "import" => {
            let src = get("src").ok_or_else(|| fail("import requires src".into()))?;
            validate_asset_path(&src).map_err(|e| fail(e.into()))?;
            if !src.ends_with(".kui") {
                return Err(fail("import src must end in .kui".into()));
            }
            if imports.insert(name.clone(), src).is_some() {
                return Err(fail(format!("duplicate import `{name}`")));
            }
        }
        "prop" => {
            if root != "component" {
                return Err(fail("prop declarations require a component root".into()));
            }
            if contract.props.iter().any(|p| p.name == name) {
                return Err(fail(format!("duplicate prop `{name}`")));
            }
            let kind = match get("type").as_deref() {
                Some("string") => KuiPropType::String,
                Some("boolean") => KuiPropType::Boolean,
                Some("integer") => KuiPropType::Integer,
                Some("number") => KuiPropType::Number,
                Some("record") => KuiPropType::Record,
                Some("sequence") => KuiPropType::Sequence,
                Some("scalar") => KuiPropType::Scalar,
                _ => return Err(fail("prop requires a supported type".into())),
            };
            let default = get("default");
            if let Some(value) = &default {
                kind.literal(value).map_err(fail)?;
            }
            contract.props.push(KuiProp {
                name,
                kind,
                default,
            });
        }
        _ => {
            if root != "component" {
                return Err(fail("event declarations require a component root".into()));
            }
            if contract.events.contains(&name) {
                return Err(fail(format!("duplicate event `{name}`")));
            }
            contract.events.push(name);
        }
    }
    Ok(true)
}

pub(super) fn parse_empty(
    reader: &XmlReader<&[u8]>,
    source: &str,
    ids: &mut HashSet<String>,
    start: BytesStart<'_>,
    tag: &str,
) -> Result<KuiNode, KuiParseError> {
    node_attrs(reader, source, ids, &start, tag)
}

pub(super) fn parse_node(
    reader: &mut XmlReader<&[u8]>,
    source: &str,
    ids: &mut HashSet<String>,
    start: BytesStart<'_>,
    tag: &str,
    inside_button: bool,
    _empty: bool,
) -> Result<KuiNode, KuiParseError> {
    let mut node = node_attrs(reader, source, ids, &start, tag)?;
    if let KuiNodeKind::Slot { children, .. } = &mut node.kind {
        *children = parse_visual_children(reader, source, ids, "slot", inside_button)?;
        return Ok(node);
    }
    let KuiNodeKind::Use {
        props,
        events,
        slots,
        ..
    } = &mut node.kind
    else {
        unreachable!()
    };
    loop {
        match reader
            .read_event()
            .map_err(|e| error(reader, source, e.to_string()))?
        {
            Event::Empty(start) => {
                let tag = element_name(&start, reader, source)?;
                let attrs = attributes(reader, source, &start)?;
                let get = |name: &str| {
                    attrs
                        .iter()
                        .find(|(k, _)| k == name)
                        .map(|(_, v)| v.clone())
                };
                let name =
                    get("name").ok_or_else(|| error(reader, source, "input requires name"))?;
                if tag == "prop" {
                    identifier(&name)
                } else {
                    label(&name)
                }
                .map_err(|e| error(reader, source, e))?;
                if tag == "prop" {
                    if attrs
                        .iter()
                        .any(|(k, _)| !["name", "value", "bind:value"].contains(&k.as_str()))
                    {
                        return Err(error(reader, source, "unknown prop input attribute"));
                    }
                    let input = match (get("value"), get("bind:value")) {
                        (Some(v), None) => KuiPropSource::Literal(v),
                        (None, Some(v)) => {
                            validate_binding_path(&v).map_err(|e| error(reader, source, e))?;
                            KuiPropSource::Binding(v)
                        }
                        _ => {
                            return Err(error(
                                reader,
                                source,
                                "prop input requires exactly one of value or bind:value",
                            ));
                        }
                    };
                    if props.insert(name.clone(), input).is_some() {
                        return Err(error(reader, source, format!("duplicate input `{name}`")));
                    }
                } else if tag == "on" {
                    if attrs
                        .iter()
                        .any(|(k, _)| !["name", "action"].contains(&k.as_str()))
                    {
                        return Err(error(reader, source, "unknown event mapping attribute"));
                    }
                    let action = get("action")
                        .filter(|v| !v.trim().is_empty())
                        .ok_or_else(|| error(reader, source, "on requires nonempty action"))?;
                    if events.insert(name.clone(), action).is_some() {
                        return Err(error(
                            reader,
                            source,
                            format!("duplicate event mapping `{name}`"),
                        ));
                    }
                } else {
                    return Err(error(reader, source, "use accepts prop, on, and fill"));
                }
            }
            Event::Start(start) => {
                require_element(&start, "fill", reader, source)?;
                let attrs = attributes(reader, source, &start)?;
                if attrs.len() != 1 || attrs[0].0 != "slot" {
                    return Err(error(reader, source, "fill requires only slot"));
                }
                let name = attrs[0].1.clone();
                label(&name).map_err(|e| error(reader, source, e))?;
                let content = parse_visual_children(reader, source, ids, "fill", inside_button)?;
                if slots.insert(name.clone(), content).is_some() {
                    return Err(error(reader, source, format!("duplicate fill `{name}`")));
                }
            }
            Event::End(end) if end.name().as_ref() == b"use" => break,
            Event::Comment(_) => {}
            Event::Text(text) if text.decode().is_ok_and(|s| s.trim().is_empty()) => {}
            _ => return Err(error(reader, source, "invalid or unclosed use")),
        }
    }
    Ok(node)
}

fn node_attrs(
    reader: &XmlReader<&[u8]>,
    source: &str,
    ids: &mut HashSet<String>,
    start: &BytesStart<'_>,
    tag: &str,
) -> Result<KuiNode, KuiParseError> {
    let mut standard = BytesStart::new("node");
    let mut name = None;
    for (key, value) in attributes(reader, source, start)? {
        if key == if tag == "use" { "component" } else { "name" } {
            name = Some(value);
        } else {
            if tag == "slot" {
                return Err(error(
                    reader,
                    source,
                    "slot accepts only name; style its authored content",
                ));
            }
            // String attributes are escaped by quick-xml when inserted.
            standard.push_attribute((key.as_str(), value.as_str()));
        }
    }
    let name = name.ok_or_else(|| error(reader, source, format!("<{tag}> requires a name")))?;
    label(&name).map_err(|e| error(reader, source, e))?;
    let visual = parse_visual_attributes(reader, source, ids, &standard, "node")?;
    Ok(KuiNode {
        attributes: visual.properties,
        id: visual.id,
        class: visual.class,
        bindings: visual.bindings,
        kind: if tag == "use" {
            KuiNodeKind::Use {
                component: name,
                props: default(),
                events: default(),
                slots: default(),
            }
        } else {
            KuiNodeKind::Slot {
                name,
                children: vec![],
            }
        },
    })
}

fn label(name: &str) -> Result<(), String> {
    if name.starts_with('-') {
        return Err("name must begin with an identifier character".into());
    }
    identifier(&name.replace('-', "_"))
}

fn identifier(name: &str) -> Result<(), String> {
    crate::path::BindingPath::parse(name).map_err(String::from)?;
    if name.contains(['.', '[', ']']) {
        return Err("name must be a single identifier".into());
    }
    Ok(())
}

pub(super) fn parse_meter(
    reader: &XmlReader<&[u8]>,
    source: &str,
    ids: &mut HashSet<String>,
    start: &BytesStart<'_>,
) -> Result<KuiNode, KuiParseError> {
    let mut standard = BytesStart::new("node");
    let mut current = None;
    let mut maximum = None;
    for (key, value) in attributes(reader, source, start)? {
        match key.as_str() {
            "bind:current" => current = Some(value),
            "bind:maximum" => maximum = Some(value),
            _ => {
                standard.push_attribute((key.as_str(), quick_xml::escape::escape(&value).as_ref()))
            }
        }
    }
    let current = current.ok_or_else(|| error(reader, source, "meter requires bind:current"))?;
    let maximum = maximum.ok_or_else(|| error(reader, source, "meter requires bind:maximum"))?;
    for path in [&current, &maximum] {
        validate_binding_path(path).map_err(|e| error(reader, source, e))?;
    }
    let visual = parse_visual_attributes(reader, source, ids, &standard, "node")?;
    Ok(KuiNode {
        attributes: visual.properties,
        id: visual.id,
        class: visual.class,
        bindings: visual.bindings,
        kind: KuiNodeKind::Meter { current, maximum },
    })
}

pub(super) trait SourceReader {
    fn read<'a>(
        &'a mut self,
        path: String,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>>;
}

impl SourceReader for LoadContext<'_> {
    fn read<'a>(
        &'a mut self,
        path: String,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>> {
        Box::pin(async move {
            self.read_asset_bytes(path)
                .await
                .map_err(|error| error.to_string())
        })
    }
}

impl SourceReader for BTreeMap<String, String> {
    fn read<'a>(
        &'a mut self,
        path: String,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>> {
        Box::pin(std::future::ready(
            self.get(&path)
                .map(|source| source.as_bytes().to_vec())
                .ok_or_else(|| format!("missing source `{path}`")),
        ))
    }
}

pub(super) fn compile<'a>(
    definition: &'a mut KuiDefinition,
    context: &'a mut (impl SourceReader + Send),
    styles: &'a mut Vec<String>,
    chain: Vec<String>,
    inherited_style: Option<String>,
) -> Pin<Box<dyn Future<Output = Result<(), KuiLoaderError>> + Send + 'a>> {
    Box::pin(async move {
        let imports = definition.imports.clone();
        let caller_style = definition
            .style
            .as_ref()
            .map(|s| s.src.clone())
            .or(inherited_style);
        expand(
            &mut definition.children,
            &imports,
            context,
            styles,
            &chain,
            caller_style,
        )
        .await?;
        validate_buttons(&definition.children, false).map_err(|message| {
            KuiLoaderError::Composition(format!("{}: {message}", chain.join(" -> ")))
        })
    })
}

fn expand<'a>(
    nodes: &'a mut [KuiNode],
    imports: &'a BTreeMap<String, String>,
    context: &'a mut (impl SourceReader + Send),
    styles: &'a mut Vec<String>,
    chain: &'a [String],
    caller_style: Option<String>,
) -> Pin<Box<dyn Future<Output = Result<(), KuiLoaderError>> + Send + 'a>> {
    Box::pin(async move {
        for node in nodes {
            match &mut node.kind {
                KuiNodeKind::Use {
                    component,
                    props,
                    events,
                    slots,
                } => {
                    let src = imports
                        .get(component)
                        .ok_or_else(|| {
                            KuiLoaderError::Composition(format!(
                                "{}: unknown component `{component}`",
                                chain.join(" -> ")
                            ))
                        })?
                        .clone();
                    if chain.contains(&src) {
                        return Err(KuiLoaderError::Composition(format!(
                            "recursive component import: {} -> {src}",
                            chain.join(" -> ")
                        )));
                    }
                    let bytes = context.read(src.clone()).await.map_err(|e| {
                        KuiLoaderError::Composition(format!("{} -> {src}: {e}", chain.join(" -> ")))
                    })?;
                    let source = String::from_utf8(bytes).map_err(KuiLoaderError::InvalidUtf8)?;
                    let mut definition = parse(&src, &source).map_err(|e| {
                        KuiLoaderError::Composition(format!("{} -> {src}: {e}", chain.join(" -> ")))
                    })?;
                    let contract = definition.component.clone().ok_or_else(|| {
                        KuiLoaderError::Composition(format!("`{src}` must have a component root"))
                    })?;
                    let fail = |msg: String| {
                        KuiLoaderError::Composition(format!(
                            "{} -> {src}: {msg}",
                            chain.join(" -> ")
                        ))
                    };
                    for name in props.keys() {
                        if !contract.props.iter().any(|p| &p.name == name) {
                            return Err(fail(format!("unknown prop `{name}`")));
                        }
                    }
                    for prop in &contract.props {
                        match props.get(&prop.name) {
                            None if prop.default.is_none() => {
                                return Err(fail(format!("missing required prop `{}`", prop.name)));
                            }
                            Some(KuiPropSource::Literal(value)) => {
                                prop.kind.literal(value).map_err(fail)?;
                            }
                            _ => {}
                        }
                    }
                    for event in events.keys() {
                        if !contract.events.contains(event) {
                            return Err(fail(format!("unknown event `{event}`")));
                        }
                    }
                    // Compile caller content before inserting it into the component's tree.
                    for content in slots.values_mut() {
                        expand(
                            content,
                            imports,
                            context,
                            styles,
                            chain,
                            caller_style.clone(),
                        )
                        .await?;
                    }
                    let mut slot_names = HashSet::new();
                    compose(
                        &mut definition.children,
                        slots,
                        events,
                        &contract.events,
                        &mut slot_names,
                        &caller_style,
                    )
                    .map_err(fail)?;
                    for name in slots.keys() {
                        if !slot_names.contains(name) {
                            return Err(fail(format!("unknown slot `{name}`")));
                        }
                    }
                    let style = definition.style.as_ref().map(|s| s.src.clone());
                    if let Some(path) = &style
                        && !styles.contains(path)
                    {
                        styles.push(path.clone());
                    }
                    let mut next_chain = chain.to_vec();
                    next_chain.push(src.clone());
                    compile(
                        &mut definition,
                        context,
                        styles,
                        next_chain,
                        caller_style.clone(),
                    )
                    .await?;
                    let mut template = definition.children.remove(0);
                    // An instance's id replaces its root's local id; classes are additive.
                    apply_identity(&mut template, node.id.take(), node.class.take());
                    node.kind = KuiNodeKind::Scope {
                        name: src,
                        props: contract.props,
                        inputs: props.clone(),
                        template: Box::new(template),
                        style,
                    };
                }
                KuiNodeKind::Node { children }
                | KuiNodeKind::Button { children, .. }
                | KuiNodeKind::Slot { children, .. } => {
                    expand(
                        children,
                        imports,
                        context,
                        styles,
                        chain,
                        caller_style.clone(),
                    )
                    .await?
                }
                KuiNodeKind::Repeat { template, .. } => {
                    expand(
                        std::slice::from_mut(template),
                        imports,
                        context,
                        styles,
                        chain,
                        caller_style.clone(),
                    )
                    .await?
                }
                _ => {}
            }
        }
        Ok(())
    })
}

pub(super) fn validate(definition: &KuiDefinition) -> Result<(), String> {
    fn walk(
        nodes: &[KuiNode],
        contract: Option<&KuiComponentContract>,
        aliases: &[String],
    ) -> Result<(), String> {
        for node in nodes {
            let mut paths: Vec<&str> = node.bindings.iter().flat_map(KuiBinding::paths).collect();
            match &node.kind {
                KuiNodeKind::Meter { current, maximum } => {
                    paths.extend([current.as_str(), maximum.as_str()]);
                }
                KuiNodeKind::Use { props, .. } => {
                    paths.extend(props.values().filter_map(|p| match p {
                        KuiPropSource::Binding(path) => Some(path.as_str()),
                        _ => None,
                    }));
                }
                KuiNodeKind::Repeat { items, .. } => paths.push(items),
                _ => {}
            }
            if let Some(contract) = contract {
                for path in paths {
                    for root in crate::expression::roots(path) {
                        if root != "props"
                            && !aliases.contains(&root)
                            && !crate::expression::environment().is_builtin(&root)
                        {
                            return Err(format!(
                                "component binding `{path}` must use declared props or local repeat aliases"
                            ));
                        }
                    }
                    // Simple paths can be checked against declared prop types.
                    // Computed expressions are evaluated in the same isolated
                    // prop/row scope, with no implicit owner access.
                    if crate::expression::roots(path).is_empty() {
                        continue;
                    }
                    let Ok(parsed) = crate::path::BindingPath::parse(path) else {
                        continue;
                    };
                    for dependency in parsed.dependencies() {
                        if aliases.contains(&dependency.root)
                            || crate::expression::environment().is_builtin(&dependency.root)
                        {
                            continue;
                        }
                        if dependency.root != "props" {
                            return Err(format!(
                                "component binding `{path}` must use declared props or local repeat aliases"
                            ));
                        }
                        let Some(crate::path::Segment::Field(name)) = dependency.segments.first()
                        else {
                            return Err(format!(
                                "component binding `{path}` must name a declared prop"
                            ));
                        };
                        let prop = contract
                            .props
                            .iter()
                            .find(|p| p.name == *name)
                            .ok_or_else(|| format!("undeclared prop in `{path}`"))?;
                        if dependency.segments.len() > 1
                            && !matches!(prop.kind, KuiPropType::Record | KuiPropType::Sequence)
                        {
                            return Err(format!("scalar prop in `{path}` has no fields"));
                        }
                    }
                }
            }
            match &node.kind {
                KuiNodeKind::Repeat {
                    alias, template, ..
                } => {
                    let mut local = aliases.to_vec();
                    local.push(alias.clone());
                    walk(std::slice::from_ref(template), contract, &local)?;
                }
                KuiNodeKind::Node { children }
                | KuiNodeKind::Button { children, .. }
                | KuiNodeKind::Slot { children, .. } => {
                    if contract.is_none() && matches!(node.kind, KuiNodeKind::Slot { .. }) {
                        return Err("slots may only be declared inside components".into());
                    }
                    walk(children, contract, aliases)?;
                }
                KuiNodeKind::Use { slots, .. } => {
                    for content in slots.values() {
                        walk(content, contract, aliases)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    walk(&definition.children, definition.component.as_ref(), &[])
}

fn validate_buttons(nodes: &[KuiNode], inside: bool) -> Result<(), String> {
    for node in nodes {
        match &node.kind {
            KuiNodeKind::Button { children, .. } => {
                if inside {
                    return Err(
                        "nested buttons are forbidden, including through components and slots"
                            .into(),
                    );
                }
                validate_buttons(children, true)?;
            }
            KuiNodeKind::Scope { template, .. } | KuiNodeKind::Repeat { template, .. } => {
                validate_buttons(std::slice::from_ref(template), inside)?
            }
            KuiNodeKind::Node { children }
            | KuiNodeKind::Slot { children, .. }
            | KuiNodeKind::Caller { children, .. } => validate_buttons(children, inside)?,
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn prepare_preview(definition: &mut KuiDefinition) -> Result<(), String> {
    if let Some(contract) = &definition.component {
        let events = contract
            .events
            .iter()
            .map(|name| (name.clone(), name.clone()))
            .collect();
        compose(
            &mut definition.children,
            &BTreeMap::new(),
            &events,
            &contract.events,
            &mut HashSet::new(),
            &None,
        )?;
    }
    Ok(())
}

fn compose(
    nodes: &mut [KuiNode],
    slots: &BTreeMap<String, Vec<KuiNode>>,
    events: &BTreeMap<String, String>,
    declared: &[String],
    names: &mut HashSet<String>,
    caller_style: &Option<String>,
) -> Result<(), String> {
    for node in nodes {
        for (key, action) in &mut node.attributes {
            if crate::interaction::is_event_attribute(key) {
                let event = action
                    .strip_prefix("emit.")
                    .ok_or("component actions must use emit.EVENT")?;
                if !declared.iter().any(|name| name == event) {
                    return Err(format!("undeclared event `{event}`"));
                }
                *action = events.get(event).cloned().unwrap_or_default();
            }
        }
        match &mut node.kind {
            KuiNodeKind::Slot { name, children } => {
                if !names.insert(name.clone()) {
                    return Err(format!("duplicate slot `{name}`"));
                }
                if let Some(content) = slots.get(name) {
                    node.kind = KuiNodeKind::Caller {
                        children: content.clone(),
                        style: caller_style.clone(),
                    };
                } else {
                    compose(children, slots, events, declared, names, caller_style)?;
                }
            }
            KuiNodeKind::Node { children } | KuiNodeKind::Button { children } => {
                compose(children, slots, events, declared, names, caller_style)?
            }
            KuiNodeKind::Repeat { template, .. } => compose(
                std::slice::from_mut(template),
                slots,
                events,
                declared,
                names,
                caller_style,
            )?,
            KuiNodeKind::Use {
                events: mappings,
                slots: nested_slots,
                ..
            } => {
                for action in mappings.values_mut() {
                    let event = action
                        .strip_prefix("emit.")
                        .ok_or("nested component mappings must forward emit.EVENT")?;
                    if !declared.iter().any(|name| name == event) {
                        return Err(format!("undeclared event `{event}`"));
                    }
                    *action = events.get(event).cloned().unwrap_or_default();
                }
                for content in nested_slots.values_mut() {
                    compose(content, slots, events, declared, names, caller_style)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn apply_identity(node: &mut KuiNode, id: Option<String>, class: Option<String>) {
    if let KuiNodeKind::Scope { template, .. } = &mut node.kind {
        apply_identity(template, id, class);
    } else {
        if id.is_some() {
            node.id = id;
        }
        if let Some(class) = class {
            node.class = Some(format!("{} {class}", node.class.take().unwrap_or_default()));
        }
    }
}
