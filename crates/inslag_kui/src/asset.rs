use std::{
    collections::HashSet,
    error::Error as StdError,
    fmt,
    path::{Component, Path},
    str,
};

use crate::{KuiTextFormat, KuiTextPart, text::parse_template};
mod components;
use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
};
use bevy_flair::prelude::StyleSheet;
pub use components::{KuiComponentContract, KuiProp, KuiPropSource, KuiPropType};
use quick_xml::{
    escape::unescape,
    events::{BytesStart, Event},
    reader::Reader as XmlReader,
};

const VIEW_NAMESPACE: &str = "urn:inslag-kui:1";
const BIND_NAMESPACE: &str = "urn:inslag-kui:binding:1";

pub(crate) const KUI_EXTENSION: &[&str] = &["kui"];

/// A parsed view or component, with its styles, imports, and element tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KuiDefinition {
    pub name: String,
    pub style: Option<KuiStyleDefinition>,
    pub children: Vec<KuiNode>,
    pub imports: std::collections::BTreeMap<String, String>,
    pub component: Option<KuiComponentContract>,
}

/// A stylesheet path relative to the asset root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KuiStyleDefinition {
    pub src: String,
}

/// An element's identity, bindings, attributes, and content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KuiNode {
    pub id: Option<String>,
    pub class: Option<String>,
    pub bindings: Vec<KuiBinding>,
    pub attributes: std::collections::BTreeMap<String, String>,
    pub kind: KuiNodeKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KuiNodeKind {
    Use {
        component: String,
        props: std::collections::BTreeMap<String, KuiPropSource>,
        events: std::collections::BTreeMap<String, String>,
        slots: std::collections::BTreeMap<String, Vec<KuiNode>>,
    },
    /// An expanded component, with bindings evaluated against its props.
    Scope {
        name: String,
        props: Vec<KuiProp>,
        inputs: std::collections::BTreeMap<String, KuiPropSource>,
        template: Box<KuiNode>,
        style: Option<String>,
    },
    Slot {
        name: String,
        children: Vec<KuiNode>,
    },
    /// Filled slot content, with bindings evaluated in the caller's scope.
    Caller {
        children: Vec<KuiNode>,
        style: Option<String>,
    },
    Meter {
        current: String,
        maximum: String,
    },
    Repeat {
        items: String,
        alias: String,
        key: String,
        template: Box<KuiNode>,
    },
    Node {
        children: Vec<KuiNode>,
    },
    Button {
        children: Vec<KuiNode>,
    },
    Text {
        value: String,
    },
    Image {
        src: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KuiBinding {
    Property { name: String, path: String },
    Value(String),
    Text(String),
    FormattedText { path: String, format: KuiTextFormat },
    TextTemplate(Vec<KuiTextPart>),
    Visible(String),
    Disabled(String),
    Attribute { name: String, expression: String },
}

impl KuiBinding {
    /// Luau expressions used by this binding, in template order.
    pub fn paths(&self) -> Vec<&str> {
        match self {
            Self::Property { path, .. } => vec![path],
            Self::Text(path)
            | Self::Visible(path)
            | Self::Disabled(path)
            | Self::Value(path)
            | Self::FormattedText { path, .. } => vec![path],
            Self::Attribute { expression, .. } => vec![expression],
            Self::TextTemplate(parts) => parts
                .iter()
                .filter_map(|part| match part {
                    KuiTextPart::Binding { path, .. } => Some(path.as_str()),
                    _ => None,
                })
                .collect(),
        }
    }
}

#[derive(Asset, Clone, Debug, TypePath)]
pub(crate) struct KuiAsset {
    pub definition: KuiDefinition,
    #[dependency]
    pub style_sheet: Option<Handle<StyleSheet>>,
    pub image_paths: Vec<String>,
    #[dependency]
    pub image_handles: Vec<Handle<Image>>,
    pub component_style_paths: Vec<String>,
    #[dependency]
    pub component_style_handles: Vec<Handle<StyleSheet>>,
}

#[derive(Default, TypePath)]
pub(crate) struct KuiAssetLoader;

#[derive(Debug)]
pub enum KuiLoaderError {
    Io(std::io::Error),
    InvalidUtf8(std::string::FromUtf8Error),
    Parse(KuiParseError),
    Composition(String),
}

impl fmt::Display for KuiLoaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "could not read KUI asset: {error}"),
            Self::InvalidUtf8(error) => write!(formatter, "KUI asset is not UTF-8: {error}"),
            Self::Parse(error) => error.fmt(formatter),
            Self::Composition(error) => formatter.write_str(error),
        }
    }
}

impl StdError for KuiLoaderError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidUtf8(error) => Some(error),
            Self::Parse(error) => Some(error),
            Self::Composition(_) => None,
        }
    }
}

impl AssetLoader for KuiAssetLoader {
    type Asset = KuiAsset;
    type Settings = ();
    type Error = KuiLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .await
            .map_err(KuiLoaderError::Io)?;
        let source = String::from_utf8(bytes).map_err(KuiLoaderError::InvalidUtf8)?;
        let source_name = load_context.path().path().display().to_string();
        let mut definition = parse(&source_name, &source).map_err(KuiLoaderError::Parse)?;
        components::prepare_preview(&mut definition).map_err(KuiLoaderError::Composition)?;
        let mut component_style_paths = Vec::new();
        components::compile(
            &mut definition,
            load_context,
            &mut component_style_paths,
            vec![source_name.clone()],
            None,
        )
        .await?;
        if let Some(contract) = &definition.component {
            let style = definition.style.as_ref().map(|s| s.src.clone());
            if let Some(path) = &style
                && !component_style_paths.contains(path)
            {
                component_style_paths.push(path.clone());
            }
            let template = definition.children.remove(0);
            definition.children.push(KuiNode {
                attributes: default(),
                id: None,
                class: None,
                bindings: vec![],
                kind: KuiNodeKind::Scope {
                    name: source_name.clone(),
                    props: contract.props.clone(),
                    inputs: contract
                        .props
                        .iter()
                        .map(|prop| {
                            (
                                prop.name.clone(),
                                KuiPropSource::Binding(format!("props.{}", prop.name)),
                            )
                        })
                        .collect(),
                    template: Box::new(template),
                    style,
                },
            });
        }
        let component_style_handles = component_style_paths
            .iter()
            .map(|path| load_context.load::<StyleSheet>(path.clone()))
            .collect();

        let style_sheet = definition
            .style
            .as_ref()
            .map(|style| load_context.load::<StyleSheet>(style.src.clone()));

        let mut image_paths = Vec::new();
        collect_image_paths(&definition.children, &mut image_paths);
        let image_handles = image_paths
            .iter()
            .map(|path| load_context.load::<Image>(path.clone()))
            .collect();

        Ok(KuiAsset {
            definition,
            style_sheet,
            image_paths,
            image_handles,
            component_style_paths,
            component_style_handles,
        })
    }

    fn extensions(&self) -> &[&str] {
        KUI_EXTENSION
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KuiParseError {
    pub source_name: String,
    pub byte_position: usize,
    pub message: String,
}

impl fmt::Display for KuiParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: byte {}: {}",
            self.source_name, self.byte_position, self.message
        )
    }
}

impl StdError for KuiParseError {}

/// Compile a view and its imported components from source files.
///
/// Keys and import paths are relative to the asset root. Returns an error for
/// missing sources, invalid KUI, or incompatible component props and slots.
///
/// ```
/// use inslag_kui::source::compile;
/// use std::collections::BTreeMap;
///
/// let sources = BTreeMap::from([
///     ("ui/menu.kui".into(), r#"
///         <view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1"
///               version="1" name="menu">
///           <import name="card" src="ui/card.kui"/>
///           <use component="card"/>
///         </view>
///     "#.into()),
///     ("ui/card.kui".into(), r#"
///         <component xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1"
///                    version="1" name="card">
///           <text>Oak</text>
///         </component>
///     "#.into()),
/// ]);
/// let view = compile("ui/menu.kui", sources)?;
/// assert_eq!(view.name, "menu");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn compile_sources(
    source_name: &str,
    mut sources: std::collections::BTreeMap<String, String>,
) -> Result<KuiDefinition, KuiLoaderError> {
    validate_asset_path(source_name)
        .map_err(|message| KuiLoaderError::Composition(message.into()))?;
    let source = sources
        .get(source_name)
        .ok_or_else(|| KuiLoaderError::Composition(format!("missing source `{source_name}`")))?;
    let mut definition = parse(source_name, source).map_err(KuiLoaderError::Parse)?;
    components::prepare_preview(&mut definition).map_err(KuiLoaderError::Composition)?;
    let mut styles = Vec::new();
    bevy::tasks::block_on(components::compile(
        &mut definition,
        &mut sources,
        &mut styles,
        vec![source_name.into()],
        None,
    ))?;
    // Component source inspection keeps its declared contract and caller-visible props.
    Ok(definition)
}

/// Parse a view or component. `source_name` identifies the file in diagnostics.
///
/// Imports remain in the returned tree; use [`crate::source::compile`] to expand them.
pub fn parse(source_name: &str, source: &str) -> Result<KuiDefinition, KuiParseError> {
    let mut reader = XmlReader::from_str(source);
    let mut ids = HashSet::new();
    let mut root = None;
    let mut saw_root = false;

    loop {
        let event = reader
            .read_event()
            .map_err(|error| xml_error(source_name, &reader, error.to_string()))?;
        match event {
            Event::Decl(_) if !saw_root => {}
            Event::Decl(_) => {
                return Err(error(
                    &reader,
                    source_name,
                    "XML declaration is not at the start",
                ));
            }
            Event::Comment(_) => {}
            Event::Text(text)
                if text
                    .decode()
                    .map(|text| text.trim().is_empty())
                    .unwrap_or(false) => {}
            Event::CData(text)
                if text
                    .decode()
                    .map(|text| text.trim().is_empty())
                    .unwrap_or(false) => {}
            Event::Start(start) if !saw_root => {
                let tag = element_name(&start, &reader, source_name)?;
                if tag != "view" && tag != "component" {
                    return Err(error(
                        &reader,
                        source_name,
                        "root element must be <view> or <component>",
                    ));
                }
                let name = parse_root_attributes(&reader, source_name, &start)?;
                let (style, children, imports, contract) =
                    parse_root_children(&mut reader, source_name, &mut ids, &tag)?;
                root = Some(KuiDefinition {
                    name,
                    style,
                    children,
                    imports,
                    component: (tag == "component").then_some(contract),
                });
                saw_root = true;
            }
            Event::Empty(start) if !saw_root => {
                require_element(&start, "view", &reader, source_name)?;
                let name = parse_root_attributes(&reader, source_name, &start)?;
                root = Some(KuiDefinition {
                    name,
                    style: None,
                    children: Vec::new(),
                    imports: default(),
                    component: None,
                });
                saw_root = true;
            }
            Event::Start(_) | Event::Empty(_) => {
                return Err(error(
                    &reader,
                    source_name,
                    "only one root <view> element is allowed",
                ));
            }
            Event::Eof if !saw_root => {
                return Err(error(&reader, source_name, "missing root <view> element"));
            }
            Event::Eof => {
                let root = root.expect("root was set before EOF");
                if root.children.len() != 1
                    || matches!(
                        root.children[0].kind,
                        KuiNodeKind::Repeat { .. } | KuiNodeKind::Slot { .. }
                    )
                {
                    return Err(error(
                        &reader,
                        source_name,
                        "a view requires exactly one visual root",
                    ));
                }
                components::validate(&root)
                    .map_err(|message| error(&reader, source_name, message))?;
                return Ok(root);
            }
            Event::Text(_) | Event::CData(_) => {
                return Err(error(
                    &reader,
                    source_name,
                    "literal text is only allowed in <text>",
                ));
            }
            Event::PI(_) => {
                return Err(error(
                    &reader,
                    source_name,
                    "processing instructions are not allowed",
                ));
            }
            Event::DocType(_) => {
                return Err(error(
                    &reader,
                    source_name,
                    "DTD declarations are not allowed",
                ));
            }
            Event::GeneralRef(_) => {
                return Err(error(
                    &reader,
                    source_name,
                    "general references are not allowed here",
                ));
            }
            Event::End(_) => return Err(error(&reader, source_name, "unexpected closing element")),
        }
    }
}

fn parse_root_children(
    reader: &mut XmlReader<&[u8]>,
    source_name: &str,
    ids: &mut HashSet<String>,
    closing: &str,
) -> Result<
    (
        Option<KuiStyleDefinition>,
        Vec<KuiNode>,
        std::collections::BTreeMap<String, String>,
        KuiComponentContract,
    ),
    KuiParseError,
> {
    let mut style = None;
    let mut children = Vec::new();
    let mut imports = std::collections::BTreeMap::new();
    let mut contract = KuiComponentContract::default();

    loop {
        let event = reader
            .read_event()
            .map_err(|error| xml_error(source_name, reader, error.to_string()))?;
        match event {
            Event::Start(start) => {
                let element = element_name(&start, reader, source_name)?;
                if element == "style" {
                    return Err(error(
                        reader,
                        source_name,
                        "<style> must be an empty element",
                    ));
                }
                if element == "view" {
                    return Err(error(
                        reader,
                        source_name,
                        "nested <view> elements are not allowed",
                    ));
                }
                if !is_visual_element(&element) {
                    return Err(error(
                        reader,
                        source_name,
                        format!("unknown element <{element}>"),
                    ));
                }
                children.push(parse_visual_start(
                    reader,
                    source_name,
                    ids,
                    start,
                    element,
                    false,
                )?);
            }
            Event::Empty(start) => {
                let element = element_name(&start, reader, source_name)?;
                if components::declaration(
                    reader,
                    source_name,
                    &start,
                    &element,
                    closing,
                    &mut imports,
                    &mut contract,
                )? {
                    continue;
                }
                if element == "style" {
                    if style.is_some() {
                        return Err(error(
                            reader,
                            source_name,
                            "only one <style> element is allowed",
                        ));
                    }
                    style = Some(parse_style_attributes(reader, source_name, &start)?);
                } else {
                    if element == "view" || !is_visual_element(&element) {
                        return Err(error(
                            reader,
                            source_name,
                            format!("unknown element <{element}>"),
                        ));
                    }
                    children.push(parse_visual_empty(
                        reader,
                        source_name,
                        ids,
                        start,
                        element,
                        false,
                    )?);
                }
            }
            Event::End(end) if end.name().as_ref() == closing.as_bytes() => {
                return Ok((style, children, imports, contract));
            }
            Event::End(_) => return Err(error(reader, source_name, "unexpected closing element")),
            Event::Text(text)
                if text
                    .decode()
                    .map(|text| text.trim().is_empty())
                    .unwrap_or(false) => {}
            Event::CData(text)
                if text
                    .decode()
                    .map(|text| text.trim().is_empty())
                    .unwrap_or(false) => {}
            Event::Comment(_) => {}
            Event::Decl(_) => {
                return Err(error(
                    reader,
                    source_name,
                    "XML declaration is not allowed here",
                ));
            }
            Event::Text(_) | Event::CData(_) => {
                return Err(error(
                    reader,
                    source_name,
                    "literal text is only allowed in <text>",
                ));
            }
            Event::PI(_) => {
                return Err(error(
                    reader,
                    source_name,
                    "processing instructions are not allowed",
                ));
            }
            Event::DocType(_) => {
                return Err(error(
                    reader,
                    source_name,
                    "DTD declarations are not allowed",
                ));
            }
            Event::GeneralRef(_) => {
                return Err(error(
                    reader,
                    source_name,
                    "general references are not allowed here",
                ));
            }
            Event::Eof => return Err(error(reader, source_name, "unclosed <view> element")),
        }
    }
}

fn parse_visual_start(
    reader: &mut XmlReader<&[u8]>,
    source_name: &str,
    ids: &mut HashSet<String>,
    start: BytesStart<'_>,
    element: String,
    inside_button: bool,
) -> Result<KuiNode, KuiParseError> {
    if element == "use" || element == "slot" {
        return components::parse_node(
            reader,
            source_name,
            ids,
            start,
            &element,
            inside_button,
            false,
        );
    }
    if element == "meter" {
        return Err(error(reader, source_name, "<meter> must be empty"));
    }
    if element == "repeat" {
        let mut items = None;
        let mut alias = None;
        let mut key_path = None;
        for (key, value) in attributes(reader, source_name, &start)? {
            match key.as_str() {
                "bind:items" => items = Some(value),
                "as" => alias = Some(value),
                "key" => key_path = Some(value),
                _ => {
                    return Err(error(
                        reader,
                        source_name,
                        format!("unknown repeat attribute `{key}`"),
                    ));
                }
            }
        }
        let items =
            items.ok_or_else(|| error(reader, source_name, "repeat requires bind:items"))?;
        let alias = alias.ok_or_else(|| error(reader, source_name, "repeat requires as"))?;
        crate::expression::environment()
            .validate_input_name(&alias)
            .map_err(|message| error(reader, source_name, message.to_string()))?;
        let key = key_path.ok_or_else(|| error(reader, source_name, "repeat requires key"))?;
        for path in [&items, &alias, &key] {
            validate_binding_path(path).map_err(|message| error(reader, source_name, message))?;
        }
        if crate::path::BindingPath::parse(&alias).is_err()
            || alias.contains(['.', '[', ']'])
            || crate::expression::roots(&alias) != [alias.clone()]
        {
            return Err(error(
                reader,
                source_name,
                "repeat alias must be a single identifier",
            ));
        }
        let mut template_ids = HashSet::new();
        let mut children = parse_visual_children(
            reader,
            source_name,
            &mut template_ids,
            "repeat",
            inside_button,
        )?;
        if children.len() != 1
            || matches!(
                children[0].kind,
                KuiNodeKind::Repeat { .. } | KuiNodeKind::Slot { .. }
            )
        {
            return Err(error(
                reader,
                source_name,
                "repeat requires exactly one visual template root",
            ));
        }
        return Ok(KuiNode {
            attributes: default(),
            id: None,
            class: None,
            bindings: vec![],
            kind: KuiNodeKind::Repeat {
                items,
                alias,
                key,
                template: Box::new(children.remove(0)),
            },
        });
    }
    if inside_button && element == "button" {
        return Err(error(
            reader,
            source_name,
            "nested <button> elements are not allowed",
        ));
    }
    let attributes = parse_visual_attributes(reader, source_name, ids, &start, &element)?;
    match element.as_str() {
        "node" => Ok(KuiNode {
            attributes: attributes.properties,
            id: attributes.id,
            class: attributes.class,
            bindings: attributes.bindings,
            kind: KuiNodeKind::Node {
                children: parse_visual_children(reader, source_name, ids, "node", inside_button)?,
            },
        }),
        "button" => Ok(KuiNode {
            attributes: attributes.properties,
            id: attributes.id,
            class: attributes.class,
            bindings: attributes.bindings,
            kind: KuiNodeKind::Button {
                children: parse_visual_children(reader, source_name, ids, "button", true)?,
            },
        }),
        "text" => Ok(KuiNode {
            attributes: attributes.properties,
            id: attributes.id,
            class: attributes.class,
            bindings: attributes.bindings,
            kind: KuiNodeKind::Text {
                value: parse_text_content(reader, source_name)?,
            },
        }),
        "image" => {
            let src = attributes
                .src
                .ok_or_else(|| error(reader, source_name, "<image> requires a src attribute"))?;
            Err(error(
                reader,
                source_name,
                format!("<image src=\"{src}\"> must be an empty element"),
            ))
        }
        _ => Err(error(
            reader,
            source_name,
            format!("unknown element <{element}>"),
        )),
    }
}

fn parse_visual_children(
    reader: &mut XmlReader<&[u8]>,
    source_name: &str,
    ids: &mut HashSet<String>,
    closing_element: &str,
    inside_button: bool,
) -> Result<Vec<KuiNode>, KuiParseError> {
    let mut children = Vec::new();
    loop {
        let event = reader
            .read_event()
            .map_err(|error| xml_error(source_name, reader, error.to_string()))?;
        match event {
            Event::Start(start) => {
                let element = element_name(&start, reader, source_name)?;
                if !is_visual_element(&element) {
                    return Err(error(
                        reader,
                        source_name,
                        format!("unknown element <{element}>"),
                    ));
                }
                children.push(parse_visual_start(
                    reader,
                    source_name,
                    ids,
                    start,
                    element,
                    inside_button,
                )?);
            }
            Event::Empty(start) => {
                let element = element_name(&start, reader, source_name)?;
                if !is_visual_element(&element) {
                    return Err(error(
                        reader,
                        source_name,
                        format!("unknown element <{element}>"),
                    ));
                }
                children.push(parse_visual_empty(
                    reader,
                    source_name,
                    ids,
                    start,
                    element,
                    inside_button,
                )?);
            }
            Event::End(end) if end.name().as_ref() == closing_element.as_bytes() => {
                return Ok(children);
            }
            Event::End(_) => return Err(error(reader, source_name, "unexpected closing element")),
            Event::Text(text)
                if text
                    .decode()
                    .map(|text| text.trim().is_empty())
                    .unwrap_or(false) => {}
            Event::CData(text)
                if text
                    .decode()
                    .map(|text| text.trim().is_empty())
                    .unwrap_or(false) => {}
            Event::Comment(_) => {}
            Event::Text(_) | Event::CData(_) => {
                return Err(error(
                    reader,
                    source_name,
                    "literal text is only allowed in <text>",
                ));
            }
            Event::Decl(_) => {
                return Err(error(
                    reader,
                    source_name,
                    "XML declaration is not allowed here",
                ));
            }
            Event::PI(_) => {
                return Err(error(
                    reader,
                    source_name,
                    "processing instructions are not allowed",
                ));
            }
            Event::DocType(_) => {
                return Err(error(
                    reader,
                    source_name,
                    "DTD declarations are not allowed",
                ));
            }
            Event::GeneralRef(_) => {
                return Err(error(
                    reader,
                    source_name,
                    "general references are not allowed here",
                ));
            }
            Event::Eof => {
                return Err(error(
                    reader,
                    source_name,
                    format!("unclosed <{closing_element}> element"),
                ));
            }
        }
    }
}

fn parse_text_content(
    reader: &mut XmlReader<&[u8]>,
    source_name: &str,
) -> Result<String, KuiParseError> {
    let mut value = String::new();
    loop {
        let event = reader
            .read_event()
            .map_err(|error| xml_error(source_name, reader, error.to_string()))?;
        match event {
            Event::Text(text) => value.push_str(&decode_text(&text, reader, source_name)?),
            Event::CData(text) => value.push_str(&decode_cdata(&text, reader, source_name)?),
            Event::GeneralRef(reference) => {
                value.push_str(&decode_reference(&reference, reader, source_name)?)
            }
            Event::Comment(_) => {}
            Event::End(end) if end.name().as_ref() == b"text" => {
                return Ok(value.split_whitespace().collect::<Vec<_>>().join(" "));
            }
            Event::End(_) => return Err(error(reader, source_name, "unexpected closing element")),
            Event::Start(_) | Event::Empty(_) => {
                return Err(error(
                    reader,
                    source_name,
                    "inline visual content is not implemented by the current text backend",
                ));
            }
            Event::Decl(_) => {
                return Err(error(
                    reader,
                    source_name,
                    "XML declaration is not allowed here",
                ));
            }
            Event::PI(_) => {
                return Err(error(
                    reader,
                    source_name,
                    "processing instructions are not allowed",
                ));
            }
            Event::DocType(_) => {
                return Err(error(
                    reader,
                    source_name,
                    "DTD declarations are not allowed",
                ));
            }
            Event::Eof => return Err(error(reader, source_name, "unclosed <text> element")),
        }
    }
}

#[derive(Default)]
struct VisualAttributes {
    properties: std::collections::BTreeMap<String, String>,
    id: Option<String>,
    class: Option<String>,
    src: Option<String>,
    value: Option<String>,
    bindings: Vec<KuiBinding>,
    format: Option<KuiTextFormat>,
}

pub(crate) fn collect_image_paths(nodes: &[KuiNode], paths: &mut Vec<String>) {
    for node in nodes {
        match &node.kind {
            KuiNodeKind::Scope { template, .. } => {
                collect_image_paths(std::slice::from_ref(template), paths)
            }
            KuiNodeKind::Slot { children, .. } | KuiNodeKind::Caller { children, .. } => {
                collect_image_paths(children, paths)
            }
            KuiNodeKind::Use { slots, .. } => {
                for children in slots.values() {
                    collect_image_paths(children, paths);
                }
            }
            KuiNodeKind::Meter { .. } => {}
            KuiNodeKind::Repeat { template, .. } => {
                collect_image_paths(std::slice::from_ref(template), paths)
            }
            KuiNodeKind::Node { children } | KuiNodeKind::Button { children, .. } => {
                collect_image_paths(children, paths);
            }
            KuiNodeKind::Image { src } if !paths.iter().any(|path| path == src) => {
                paths.push(src.clone());
            }
            KuiNodeKind::Image { .. } | KuiNodeKind::Text { .. } => {}
        }
    }
}

fn xml_error(source_name: &str, reader: &XmlReader<&[u8]>, message: String) -> KuiParseError {
    KuiParseError {
        source_name: source_name.to_owned(),
        byte_position: reader.error_position().max(reader.buffer_position()) as usize,
        message: format!("invalid XML: {message}"),
    }
}

fn require_element(
    start: &BytesStart<'_>,
    expected: &str,
    reader: &XmlReader<&[u8]>,
    source_name: &str,
) -> Result<(), KuiParseError> {
    if element_name(start, reader, source_name)? != expected {
        return Err(error(
            reader,
            source_name,
            format!("root element must be <{expected}>"),
        ));
    }
    Ok(())
}

fn element_name(
    start: &BytesStart<'_>,
    reader: &XmlReader<&[u8]>,
    source_name: &str,
) -> Result<String, KuiParseError> {
    let raw_name = start.name();
    let name = str::from_utf8(raw_name.as_ref()).map_err(|parse_error| {
        error(
            reader,
            source_name,
            format!("element name is not UTF-8: {parse_error}"),
        )
    })?;
    if name.contains(':') {
        return Err(error(
            reader,
            source_name,
            format!("namespaced element `{name}` is not allowed"),
        ));
    }
    Ok(name.to_owned())
}

fn parse_style_attributes(
    reader: &XmlReader<&[u8]>,
    source_name: &str,
    start: &BytesStart<'_>,
) -> Result<KuiStyleDefinition, KuiParseError> {
    let attributes = attributes(reader, source_name, start)?;
    if attributes.len() != 1 || attributes[0].0 != "src" {
        return Err(error(reader, source_name, "<style> accepts only src"));
    }
    validate_asset_path(&attributes[0].1)
        .map_err(|message| error(reader, source_name, format!("invalid style src: {message}")))?;
    Ok(KuiStyleDefinition {
        src: attributes[0].1.clone(),
    })
}

fn parse_visual_empty(
    reader: &XmlReader<&[u8]>,
    source_name: &str,
    ids: &mut HashSet<String>,
    start: BytesStart<'_>,
    element: String,
    inside_button: bool,
) -> Result<KuiNode, KuiParseError> {
    if element == "use" || element == "slot" {
        return components::parse_empty(reader, source_name, ids, start, &element);
    }
    if element == "meter" {
        return components::parse_meter(reader, source_name, ids, &start);
    }
    if inside_button && element == "button" {
        return Err(error(
            reader,
            source_name,
            "nested <button> elements are not allowed",
        ));
    }
    let attributes = parse_visual_attributes(reader, source_name, ids, &start, &element)?;
    match element.as_str() {
        "node" => Ok(KuiNode {
            attributes: attributes.properties,
            id: attributes.id,
            class: attributes.class,
            bindings: attributes.bindings,
            kind: KuiNodeKind::Node {
                children: Vec::new(),
            },
        }),
        "button" => Ok(KuiNode {
            attributes: attributes.properties,
            id: attributes.id,
            class: attributes.class,
            bindings: attributes.bindings,
            kind: KuiNodeKind::Button {
                children: Vec::new(),
            },
        }),
        "text" => Ok(KuiNode {
            attributes: attributes.properties,
            id: attributes.id,
            class: attributes.class,
            bindings: attributes.bindings,
            kind: KuiNodeKind::Text {
                value: String::new(),
            },
        }),
        "image" => Ok(KuiNode {
            attributes: attributes.properties,
            id: attributes.id,
            class: attributes.class,
            bindings: attributes.bindings,
            kind: KuiNodeKind::Image {
                src: attributes.src.ok_or_else(|| {
                    error(reader, source_name, "<image> requires a src attribute")
                })?,
            },
        }),
        _ => Err(error(
            reader,
            source_name,
            format!("unknown element <{element}>"),
        )),
    }
}

fn parse_visual_attributes(
    reader: &XmlReader<&[u8]>,
    source_name: &str,
    ids: &mut HashSet<String>,
    start: &BytesStart<'_>,
    element: &str,
) -> Result<VisualAttributes, KuiParseError> {
    let mut visual = VisualAttributes::default();
    for (key, value) in attributes(reader, source_name, start)? {
        match key.as_str() {
            "id" => {
                if value.is_empty() {
                    return Err(error(reader, source_name, "id must not be empty"));
                }
                if !ids.insert(value.clone()) {
                    return Err(error(
                        reader,
                        source_name,
                        format!("duplicate id `{value}`"),
                    ));
                }
                visual.id = Some(value);
            }
            "class" => visual.class = Some(value),
            "tab-index" => {
                if !value.parse::<i32>().is_ok_and(|index| index >= -1) {
                    return Err(error(
                        reader,
                        source_name,
                        "tab-index requires an integer >= -1",
                    ));
                }
                visual.properties.insert(key, value);
            }
            "format" if element == "text" => {
                visual.format = Some(
                    KuiTextFormat::parse(&value)
                        .map_err(|message| error(reader, source_name, message))?,
                );
            }
            "template" if element == "text" => {
                visual.bindings.push(KuiBinding::TextTemplate(
                    parse_template(&value)
                        .map_err(|message| error(reader, source_name, message))?,
                ));
            }
            "src" if element == "image" => {
                validate_asset_path(&value).map_err(|message| {
                    error(reader, source_name, format!("invalid image src: {message}"))
                })?;
                visual.src = Some(value);
            }
            "src" => return Err(error(reader, source_name, "src is only allowed on <image>")),
            key if crate::interaction::is_event_attribute(key) => {
                if value.trim().is_empty() {
                    return Err(error(reader, source_name, "action must not be empty"));
                }
                visual.properties.insert(key.into(), value);
            }
            "value" => {
                visual.properties.insert(key, value.clone());
                visual.value = Some(value);
            }
            "cursor" if matches!(value.as_str(), "default" | "pointer" | "grab") => {
                visual.properties.insert(key, value);
            }
            "crop" if element == "image" => {
                let values = value
                    .split_whitespace()
                    .map(str::parse::<f32>)
                    .collect::<Result<Vec<_>, _>>();
                if !values.as_ref().is_ok_and(|v| {
                    v.len() == 4 && v.iter().all(|n| n.is_finite()) && v[2] > v[0] && v[3] > v[1]
                }) {
                    return Err(error(
                        reader,
                        source_name,
                        "crop requires min-x min-y max-x max-y with positive area",
                    ));
                }
                visual.properties.insert(key, value);
            }
            "x" | "y" | "z" | "scale-x" | "scale-y" => {
                if !value.parse::<f32>().is_ok_and(f32::is_finite) {
                    return Err(error(
                        reader,
                        source_name,
                        format!("{key} requires a finite number"),
                    ));
                }
                visual.properties.insert(key, value);
            }
            "bind:x" | "bind:y" | "bind:z" | "bind:scale-x" | "bind:scale-y" | "bind:position"
            | "bind:scale" | "bind:rotation" | "bind:bottom" | "bind:right" => {
                validate_binding_path(&value)
                    .map_err(|message| error(reader, source_name, message))?;
                visual.bindings.push(KuiBinding::Property {
                    name: key[5..].into(),
                    path: value,
                });
            }
            "bind:value" => {
                validate_binding_path(&value)
                    .map_err(|message| error(reader, source_name, message))?;
                visual.bindings.push(KuiBinding::Value(value));
            }
            "bind:text" if element == "text" => {
                validate_binding_path(&value).map_err(|message| {
                    error(
                        reader,
                        source_name,
                        format!("invalid text binding: {message}"),
                    )
                })?;
                visual.bindings.push(KuiBinding::Text(value));
            }
            "bind:text" => {
                return Err(error(
                    reader,
                    source_name,
                    "bind:text is only allowed on <text>",
                ));
            }
            "bind:visible" => {
                validate_binding_path(&value).map_err(|message| {
                    error(
                        reader,
                        source_name,
                        format!("invalid visibility binding: {message}"),
                    )
                })?;
                visual.bindings.push(KuiBinding::Visible(value));
            }
            "bind:disabled" => {
                validate_binding_path(&value).map_err(|message| {
                    error(
                        reader,
                        source_name,
                        format!("invalid disabled binding: {message}"),
                    )
                })?;
                visual.bindings.push(KuiBinding::Disabled(value));
            }
            key if key.strip_prefix("bind:attr-").is_some() => {
                let name = key.strip_prefix("bind:attr-").unwrap();
                if name.is_empty() || name.contains(':') {
                    return Err(error(
                        reader,
                        source_name,
                        "bind:attr-* requires a valid attribute name",
                    ));
                }
                validate_binding_path(&value).map_err(|message| {
                    error(
                        reader,
                        source_name,
                        format!("invalid attribute binding: {message}"),
                    )
                })?;
                visual.bindings.push(KuiBinding::Attribute {
                    name: name.to_owned(),
                    expression: value,
                });
            }
            _ => {
                return Err(error(
                    reader,
                    source_name,
                    format!("unknown attribute `{key}` on <{element}>"),
                ));
            }
        }
    }
    for (compound, members) in [
        ("position", &["x", "y", "z"][..]),
        ("scale", &["scale-x", "scale-y"][..]),
    ] {
        if visual.bindings.iter().any(|b| matches!(b, KuiBinding::Property { name, .. } if name == compound))
            && (visual.bindings.iter().any(|b| matches!(b, KuiBinding::Property { name, .. } if members.contains(&name.as_str())))
                || members.iter().any(|name| visual.properties.contains_key(*name))) {
            return Err(error(reader, source_name, format!("bind:{compound} conflicts with its scalar properties")));
        }
    }
    if visual.value.is_some()
        && visual
            .bindings
            .iter()
            .any(|binding| matches!(binding, KuiBinding::Value(_)))
    {
        return Err(error(
            reader,
            source_name,
            "value and bind:value are mutually exclusive",
        ));
    }
    let has_text = visual
        .bindings
        .iter()
        .any(|binding| matches!(binding, KuiBinding::Text(_)));
    let has_template = visual
        .bindings
        .iter()
        .any(|binding| matches!(binding, KuiBinding::TextTemplate(_)));
    if has_template && (has_text || visual.format.is_some()) {
        return Err(error(
            reader,
            source_name,
            "template cannot be combined with bind:text or format",
        ));
    }
    if let Some(format) = visual.format.take() {
        let Some(binding) = visual
            .bindings
            .iter_mut()
            .find(|binding| matches!(binding, KuiBinding::Text(_)))
        else {
            return Err(error(reader, source_name, "format requires bind:text"));
        };
        let KuiBinding::Text(path) = binding else {
            unreachable!()
        };
        *binding = KuiBinding::FormattedText {
            path: path.clone(),
            format,
        };
    }
    Ok(visual)
}

fn parse_root_attributes(
    reader: &XmlReader<&[u8]>,
    source_name: &str,
    start: &BytesStart<'_>,
) -> Result<String, KuiParseError> {
    let mut name = None;
    let mut version = None;
    let mut view_namespace = false;
    let mut bind_namespace = false;
    for (key, value) in attributes(reader, source_name, start)? {
        match key.as_str() {
            "xmlns" => {
                if value != VIEW_NAMESPACE {
                    return Err(error(
                        reader,
                        source_name,
                        "the view namespace URI is invalid",
                    ));
                }
                view_namespace = true;
            }
            "xmlns:bind" => {
                if value != BIND_NAMESPACE {
                    return Err(error(
                        reader,
                        source_name,
                        "the bind namespace URI is invalid",
                    ));
                }
                bind_namespace = true;
            }
            "version" => version = Some(value),
            "name" => name = Some(value),
            _ => {
                return Err(error(
                    reader,
                    source_name,
                    format!("unknown root attribute `{key}`"),
                ));
            }
        }
    }
    if !view_namespace || !bind_namespace {
        return Err(error(
            reader,
            source_name,
            "root requires xmlns and xmlns:bind declarations",
        ));
    }
    if version.as_deref() != Some("1") {
        return Err(error(reader, source_name, "root version must be `1`"));
    }
    let name = name.ok_or_else(|| error(reader, source_name, "root requires a name attribute"))?;
    if name.is_empty() {
        return Err(error(reader, source_name, "root name must not be empty"));
    }
    Ok(name)
}

fn attributes(
    reader: &XmlReader<&[u8]>,
    source_name: &str,
    start: &BytesStart<'_>,
) -> Result<Vec<(String, String)>, KuiParseError> {
    let mut seen = HashSet::new();
    let mut parsed = Vec::new();
    for attribute in start.attributes().with_checks(true) {
        let attribute = attribute.map_err(|parse_error| {
            error(
                reader,
                source_name,
                format!("invalid attribute: {parse_error}"),
            )
        })?;
        let key = str::from_utf8(attribute.key.as_ref())
            .map_err(|parse_error| {
                error(
                    reader,
                    source_name,
                    format!("attribute name is not UTF-8: {parse_error}"),
                )
            })?
            .to_owned();
        if !seen.insert(key.clone()) {
            return Err(error(
                reader,
                source_name,
                format!("duplicate attribute `{key}`"),
            ));
        }
        let value = attribute
            .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|parse_error| {
                error(
                    reader,
                    source_name,
                    format!("invalid value for `{key}`: {parse_error}"),
                )
            })?
            .into_owned();
        parsed.push((key, value));
    }
    Ok(parsed)
}

fn decode_text(
    text: &quick_xml::events::BytesText<'_>,
    reader: &XmlReader<&[u8]>,
    source_name: &str,
) -> Result<String, KuiParseError> {
    let decoded = text.decode().map_err(|parse_error| {
        error(reader, source_name, format!("invalid text: {parse_error}"))
    })?;
    unescape(decoded.as_ref())
        .map(|text| text.into_owned())
        .map_err(|parse_error| {
            error(
                reader,
                source_name,
                format!("invalid text escape: {parse_error}"),
            )
        })
}

fn decode_cdata(
    text: &quick_xml::events::BytesCData<'_>,
    reader: &XmlReader<&[u8]>,
    source_name: &str,
) -> Result<String, KuiParseError> {
    text.decode()
        .map(|text| text.into_owned())
        .map_err(|parse_error| error(reader, source_name, format!("invalid CDATA: {parse_error}")))
}

fn decode_reference(
    reference: &quick_xml::events::BytesRef<'_>,
    reader: &XmlReader<&[u8]>,
    source_name: &str,
) -> Result<String, KuiParseError> {
    let name = reference.decode().map_err(|parse_error| {
        error(
            reader,
            source_name,
            format!("invalid character reference: {parse_error}"),
        )
    })?;
    match name.as_ref() {
        "amp" => Ok("&".to_owned()),
        "apos" => Ok("'".to_owned()),
        "gt" => Ok(">".to_owned()),
        "lt" => Ok("<".to_owned()),
        "quot" => Ok("\"".to_owned()),
        _ => reference
            .resolve_char_ref()
            .map_err(|parse_error| {
                error(
                    reader,
                    source_name,
                    format!("invalid character reference: {parse_error}"),
                )
            })?
            .map(|character| character.to_string())
            .ok_or_else(|| {
                error(
                    reader,
                    source_name,
                    "custom entity references are not allowed",
                )
            }),
    }
}

fn error(
    reader: &XmlReader<&[u8]>,
    source_name: &str,
    message: impl Into<String>,
) -> KuiParseError {
    KuiParseError {
        source_name: source_name.to_owned(),
        byte_position: reader.buffer_position() as usize,
        message: message.into(),
    }
}

pub fn validate_asset_path(path: &str) -> Result<(), &'static str> {
    if path.is_empty() {
        return Err("path must not be empty");
    }
    if path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path.contains("://")
        || path.contains('#')
        || (path.len() >= 2 && path.as_bytes()[1] == b':')
    {
        return Err("path must be relative");
    }
    let path = Path::new(path);
    if path.is_absolute() {
        return Err("path must be relative");
    }
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err("parent traversal is not allowed");
        }
    }
    Ok(())
}

pub(crate) fn validate_binding_path(path: &str) -> Result<(), &'static str> {
    crate::expression::environment()
        .cached_expression(path, false)
        .map(|_| ())
        .map_err(|_| "invalid binding expression")
}

fn is_visual_element(element: &str) -> bool {
    matches!(
        element,
        "node" | "button" | "text" | "image" | "repeat" | "use" | "slot" | "meter"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = r#"<view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="hud">"#;

    #[test]
    fn requires_inslag_namespaces() {
        let source = format!(r#"{HEADER}<text bind:text="title"/></view>"#);
        assert!(parse("hud.kui", &source).is_ok());
        for (namespace, legacy) in [
            ("urn:inslag-kui:1", "urn:bevy-kui:1"),
            ("urn:inslag-kui:binding:1", "urn:bevy-kui:binding:1"),
        ] {
            assert!(parse("legacy.kui", &source.replace(namespace, legacy)).is_err());
        }
    }

    #[test]
    fn parses_visual_structure_and_bindings() {
        let source = format!(
            r#"{HEADER}<style src="ui/hud.css"/><node id="root" class="panel" bind:visible="ui.open"><text id="title" bind:text="ui.title">  Crown
            watch  </text><image id="icon" src="ui/icon.png" bind:attr-alt="ui.alt"/></node></view>"#
        );
        let definition = parse("hud.kui", &source).expect("valid KUI should parse");

        assert_eq!(definition.name, "hud");
        assert_eq!(
            definition.style.as_ref().map(|style| style.src.as_str()),
            Some("ui/hud.css")
        );
        let KuiNodeKind::Node { children } = &definition.children[0].kind else {
            panic!("expected node root");
        };
        assert_eq!(definition.children[0].id.as_deref(), Some("root"));
        assert_eq!(definition.children[0].class.as_deref(), Some("panel"));
        assert_eq!(
            definition.children[0].bindings,
            [KuiBinding::Visible("ui.open".into())]
        );
        assert!(matches!(
            &children[0].kind,
            KuiNodeKind::Text { value } if value == "Crown watch"
        ));
        assert_eq!(
            children[1].bindings,
            [KuiBinding::Attribute {
                name: "alt".into(),
                expression: "ui.alt".into(),
            }]
        );
    }

    #[test]
    fn rejects_unknown_attribute() {
        let source = format!(r#"{HEADER}<node nope="x"/></view>"#);
        let error = parse("unknown.kui", &source).expect_err("unknown attributes must fail");
        assert!(error.to_string().contains("unknown attribute `nope`"));
    }

    #[test]
    fn rejects_duplicate_ids() {
        let source = format!(r#"{HEADER}<node id="same"/><text id="same"/></view>"#);
        let error = parse("duplicate.kui", &source).expect_err("duplicate IDs must fail");
        assert!(error.to_string().contains("duplicate id `same`"));
    }

    #[test]
    fn rejects_asset_path_traversal() {
        let source = format!(r#"{HEADER}<image src="ui/../secret.png"/></view>"#);
        let error = parse("path.kui", &source).expect_err("parent traversal must fail");
        assert!(
            error
                .to_string()
                .contains("parent traversal is not allowed")
        );
    }

    #[test]
    fn rejects_asset_sources_and_multiple_roots() {
        let source = format!(r#"{HEADER}<image src="mods://icon.png"/></view>"#);
        let error = parse("source.kui", &source).expect_err("asset sources must fail");
        assert!(error.to_string().contains("path must be relative"));

        let source = format!(r#"{HEADER}<node/><node/></view>"#);
        let error = parse("roots.kui", &source).expect_err("multiple roots must fail");
        assert!(error.to_string().contains("exactly one visual root"));
    }

    #[test]
    fn parses_button_actions_and_visual_children() {
        let source = format!(
            r#"{HEADER}<button id="choice" class="primary" on-click="choose" value="oak" bind:visible="ui.open" bind:disabled="ui.locked" bind:attr-aria-label="ui.label"><node><text bind:text="ui.caption">Choose</text><image src="ui/oak.png"/></node></button></view>"#
        );
        let definition = parse("button.kui", &source).expect("valid button should parse");
        let button = &definition.children[0];
        let KuiNodeKind::Button { children } = &button.kind else {
            panic!("expected button root");
        };

        assert_eq!(button.id.as_deref(), Some("choice"));
        assert_eq!(button.class.as_deref(), Some("primary"));
        assert_eq!(button.attributes["on-click"], "choose");
        assert_eq!(button.attributes["value"], "oak");
        assert_eq!(
            button.bindings,
            [
                KuiBinding::Visible("ui.open".into()),
                KuiBinding::Disabled("ui.locked".into()),
                KuiBinding::Attribute {
                    name: "aria-label".into(),
                    expression: "ui.label".into(),
                },
            ]
        );
        assert!(matches!(&children[0].kind, KuiNodeKind::Node { children } if children.len() == 2));
    }

    #[test]
    fn rejects_invalid_button_syntax() {
        for (body, expected) in [
            ("<button on-click=\"\"/>", "action must not be empty"),
            ("<button action=\"old\"/>", "unknown attribute"),
            ("<node on-unknown=\"go\"/>", "unknown attribute"),
            ("<node drag-action=\"old\"/>", "unknown attribute"),
            ("<node drag-min-x=\"0\"/>", "unknown attribute"),
            (
                "<button on-click=\"outer\"><button on-click=\"inner\"/></button>",
                "nested <button>",
            ),
        ] {
            let source = format!("{HEADER}{body}</view>");
            let error = parse("invalid-button.kui", &source)
                .expect_err("invalid button syntax must be rejected");
            assert!(
                error.to_string().contains(expected),
                "expected `{expected}` in `{error}`"
            );
        }
    }
}
