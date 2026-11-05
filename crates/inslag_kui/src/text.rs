use crate::KuiValue;

/// Formatting for `bind:text` and `{expression:format}` substitutions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum KuiTextFormat {
    /// Display a string, boolean, or number as text.
    #[default]
    Plain,
    /// Separate groups of three digits with commas: `12,345`.
    Grouped,
    /// Use the given number of decimal places: `fixed.2` gives `12.50`.
    Fixed(u8),
    /// Multiply by 100 and append `%`: `percent.1` gives `12.5%` for `0.125`.
    Percent(u8),
}

impl KuiTextFormat {
    pub(crate) fn parse(value: &str) -> Result<Self, &'static str> {
        match value {
            "plain" => Ok(Self::Plain),
            "grouped" => Ok(Self::Grouped),
            _ => {
                let (kind, precision) = value.split_once('.').unwrap_or((value, "0"));
                let precision = precision
                    .parse::<u8>()
                    .map_err(|_| "invalid text precision")?;
                if precision > 12 {
                    return Err("text precision must be at most 12");
                }
                match kind {
                    "fixed" => Ok(Self::Fixed(precision)),
                    "percent" => Ok(Self::Percent(precision)),
                    _ => Err("text format must be plain, grouped, fixed.N, or percent.N"),
                }
            }
        }
    }

    pub(crate) fn render(&self, value: &KuiValue) -> Option<String> {
        if *self == Self::Plain {
            return match value {
                KuiValue::String(value) => Some(value.clone()),
                KuiValue::Boolean(value) => Some(value.to_string()),
                KuiValue::Integer(value) => Some(value.to_string()),
                KuiValue::Number(value) if value.is_finite() => Some(value.to_string()),
                _ => None,
            };
        }
        let plain = match value {
            KuiValue::Integer(value) => value.to_string(),
            KuiValue::Number(value) if value.is_finite() => value.to_string(),
            _ => return None,
        };
        match self {
            Self::Grouped => {
                let (integer, fraction) = plain.split_once('.').unwrap_or((&plain, ""));
                let mut result = String::new();
                let digits = integer.strip_prefix('-').unwrap_or(integer);
                if integer.starts_with('-') {
                    result.push('-');
                }
                for (index, digit) in digits.chars().enumerate() {
                    if index > 0 && (digits.len() - index).is_multiple_of(3) {
                        result.push(',');
                    }
                    result.push(digit);
                }
                if !fraction.is_empty() {
                    result.push('.');
                    result.push_str(fraction);
                }
                Some(result)
            }
            Self::Fixed(precision) | Self::Percent(precision) => {
                // Preserve all integer digits for fixed integer formatting.
                if let KuiValue::Integer(value) = value
                    && matches!(self, Self::Fixed(_))
                {
                    return Some(if *precision == 0 {
                        value.to_string()
                    } else {
                        format!("{value}.{}", "0".repeat(*precision as usize))
                    });
                }
                let number = match value {
                    KuiValue::Integer(value) => *value as f64,
                    KuiValue::Number(value) => *value,
                    _ => unreachable!(),
                };
                let number = if matches!(self, Self::Percent(_)) {
                    number * 100.0
                } else {
                    number
                };
                if !number.is_finite() {
                    return None;
                }
                let suffix = if matches!(self, Self::Percent(_)) {
                    "%"
                } else {
                    ""
                };
                Some(format!(
                    "{number:.precision$}{suffix}",
                    precision = *precision as usize
                ))
            }
            Self::Plain => unreachable!(),
        }
    }
}

/// A literal or expression in a parsed text template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KuiTextPart {
    Literal(String),
    Binding { path: String, format: KuiTextFormat },
}

/// Parse `{expression}` substitutions with optional `:grouped`, `:fixed.N`, or `:percent.N` formats.
///
/// Write `{{` and `}}` for literal braces.
///
/// ```
/// use inslag_kui::source::{KuiTextFormat, KuiTextPart, parse_text_template};
///
/// let parts = parse_text_template("Coins: {coins:grouped}")?;
/// assert_eq!(parts, [
///     KuiTextPart::Literal("Coins: ".into()),
///     KuiTextPart::Binding { path: "coins".into(), format: KuiTextFormat::Grouped },
/// ]);
/// # Ok::<(), &'static str>(())
/// ```
pub fn parse_template(source: &str) -> Result<Vec<KuiTextPart>, &'static str> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut offset = 0;
    while offset < source.len() {
        let character = source[offset..].chars().next().unwrap();
        offset += character.len_utf8();
        match character {
            '{' if source[offset..].starts_with('{') => {
                offset += 1;
                literal.push('{');
            }
            '}' if source[offset..].starts_with('}') => {
                offset += 1;
                literal.push('}');
            }
            '{' => {
                if !literal.is_empty() {
                    parts.push(KuiTextPart::Literal(std::mem::take(&mut literal)));
                }
                let (end, colons) = template_end(source, offset)?;
                let binding = &source[offset..end];
                let (path, format) = colons
                    .into_iter()
                    .rev()
                    .find_map(|colon| {
                        let colon = colon - offset;
                        KuiTextFormat::parse(&binding[colon + 1..])
                            .ok()
                            .map(|format| (&binding[..colon], format))
                    })
                    .unwrap_or((binding, KuiTextFormat::Plain));
                offset = end + 1;
                crate::asset::validate_binding_path(path)?;
                parts.push(KuiTextPart::Binding {
                    path: path.into(),
                    format,
                });
            }
            '}' => return Err("unmatched text template closing brace"),
            character => literal.push(character),
        }
    }
    if !literal.is_empty() {
        parts.push(KuiTextPart::Literal(literal));
    }
    Ok(parts)
}

// Find a placeholder boundary without treating Lua table braces, quoted text,
// long strings/comments or method-call colons as template punctuation.
fn template_end(source: &str, mut offset: usize) -> Result<(usize, Vec<usize>), &'static str> {
    let bytes = source.as_bytes();
    let mut stack = Vec::new();
    let mut colons = Vec::new();
    while offset < bytes.len() {
        let byte = bytes[offset];
        if matches!(byte, b'\'' | b'"' | b'`') {
            let quote = byte;
            offset += 1;
            while offset < bytes.len() && bytes[offset] != quote {
                offset += if bytes[offset] == b'\\' { 2 } else { 1 };
            }
            offset += 1;
            continue;
        }
        let comment = source[offset..].starts_with("--");
        let start = offset + if comment { 2 } else { 0 };
        if bytes.get(start) == Some(&b'[') {
            let mut end = start + 1;
            while bytes.get(end) == Some(&b'=') {
                end += 1;
            }
            if bytes.get(end) == Some(&b'[') {
                let closing = format!("]{}]", "=".repeat(end - start - 1));
                let content = end + 1;
                offset = content
                    + source[content..]
                        .find(&closing)
                        .ok_or("unclosed Lua long string")?
                    + closing.len();
                continue;
            }
        }
        if comment {
            offset += source[offset..]
                .find('\n')
                .ok_or("unclosed text template placeholder")?
                + 1;
            continue;
        }
        match byte {
            b'(' => stack.push(b')'),
            b'[' => stack.push(b']'),
            b'{' => stack.push(b'}'),
            b'}' if stack.is_empty() => return Ok((offset, colons)),
            b')' | b']' | b'}' if stack.pop() != Some(byte) => {
                return Err("unbalanced template expression");
            }
            b':' if stack.is_empty()
                && bytes.get(offset + 1) != Some(&b':')
                && (offset == 0 || bytes[offset - 1] != b':') =>
            {
                colons.push(offset)
            }
            _ => {}
        }
        offset += source[offset..].chars().next().unwrap().len_utf8();
    }
    Err("unclosed text template placeholder")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expression_templates_keep_lua_strings_tables_and_method_calls_intact() {
        for source in [
            "Value {({x=7}).x:fixed.1}",
            "{if ready then '}' else '{'}",
            "{('hello'):upper()}",
            "{string.len([=[}]=])}",
            "{7 -- } is a comment\n:grouped}",
        ] {
            assert!(parse_template(source).is_ok(), "{source}");
        }
    }

    #[test]
    fn formats_numbers_without_losing_integer_precision() {
        for (format, value, expected) in [
            (
                KuiTextFormat::Grouped,
                KuiValue::Integer(i64::MIN),
                "-9,223,372,036,854,775,808",
            ),
            (
                KuiTextFormat::Grouped,
                KuiValue::Number(-1234.25),
                "-1,234.25",
            ),
            (
                KuiTextFormat::Fixed(2),
                KuiValue::Integer(i64::MAX),
                "9223372036854775807.00",
            ),
            (KuiTextFormat::Fixed(2), KuiValue::Number(1.256), "1.26"),
            (KuiTextFormat::Percent(0), KuiValue::Number(0.78), "78%"),
            (KuiTextFormat::Plain, KuiValue::Number(0.5), "0.5"),
        ] {
            assert_eq!(format.render(&value).as_deref(), Some(expected));
        }
        for value in [
            KuiValue::Nil,
            KuiValue::Number(f64::NAN),
            KuiValue::Number(f64::INFINITY),
            KuiValue::Sequence(vec![]),
        ] {
            assert_eq!(KuiTextFormat::Plain.render(&value), None);
        }
        assert_eq!(
            KuiTextFormat::Grouped.render(&KuiValue::String("1234".into())),
            None
        );
        assert_eq!(
            KuiTextFormat::Percent(0).render(&KuiValue::Number(f64::MAX)),
            None
        );
    }

    #[test]
    fn templates_validate_paths_formats_and_literal_braces() {
        assert_eq!(
            parse_template("{{{value:fixed.2}}}").unwrap(),
            vec![
                KuiTextPart::Literal("{".into()),
                KuiTextPart::Binding {
                    path: "value".into(),
                    format: KuiTextFormat::Fixed(2)
                },
                KuiTextPart::Literal("}".into()),
            ]
        );
        for source in [
            "{value",
            "value}",
            "{}",
            "{value:fixed.13}",
            "{value:unknown}",
            "{value:fixed.-1}",
        ] {
            assert!(parse_template(source).is_err(), "{source}");
        }
    }
}
