use inslag_format_loot::{Entry, EntryKind, ExpressionKind, Literal, Span, parse};
use proptest::prelude::*;

fn check_spans(input: &str) {
    match parse(input) {
        Ok(document) => {
            for table in document.tables {
                valid_span(input, &table.span);
                valid_span(input, &table.name.span);
                if let Some(using) = &table.using {
                    valid_span(input, &using.span);
                }
                for argument in table.defaults {
                    valid_span(input, &argument.span);
                    valid_span(input, &argument.name.span);
                    valid_span(input, &argument.value.span);
                }
                entry_spans(input, &table.entries);
            }
        }
        Err(error) => valid_span(input, &error.span),
    }
}

fn entry_spans(input: &str, entries: &[Entry]) {
    for entry in entries {
        valid_span(input, &entry.span);
        for span in entry
            .condition
            .iter()
            .map(|value| &value.span)
            .chain(entry.weight.iter().map(|value| &value.span))
        {
            valid_span(input, span);
        }
        if let Some(binding) = &entry.binding {
            valid_span(input, &binding.span);
        }
        match &entry.kind {
            EntryKind::All(children) => entry_spans(input, children),
            EntryKind::Choose {
                entries,
                rolls,
                distinct,
            } => {
                entry_spans(input, entries);
                if let Some(rolls) = rolls {
                    valid_span(input, &rolls.span);
                }
                if let Some(distinct) = distinct {
                    valid_span(input, &distinct.span);
                }
            }
            EntryKind::Call {
                function,
                arguments,
            }
            | EntryKind::Table {
                table: function,
                arguments,
            } => {
                valid_span(input, &function.span);
                for argument in arguments {
                    valid_span(input, &argument.span);
                    valid_span(input, &argument.name.span);
                    valid_span(input, &argument.value.span);
                }
            }
            EntryKind::Implicit { primary, arguments } => {
                if let Some(primary) = primary {
                    valid_span(input, &primary.span);
                }
                for argument in arguments {
                    valid_span(input, &argument.span);
                    valid_span(input, &argument.name.span);
                    valid_span(input, &argument.value.span);
                }
            }
            EntryKind::Nothing => {}
        }
    }
}

fn valid_span(input: &str, span: &Span) {
    assert!(
        span.start <= span.end && input.get(span.clone()).is_some(),
        "{span:?}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_unicode_never_panics_and_always_has_valid_spans(input in any::<String>()) {
        check_spans(&input);
        check_spans(&format!("table \"root\"\n  {input}"));
        check_spans(&format!("table \"root\" using=actor\n  @actor value={input}"));
    }

    #[test]
    fn quoted_unicode_decodes_exactly(value in any::<String>()) {
        for quote in ['\'', '"'] {
            let escaped = value.chars().map(|c| match c {
                '\n' => "\\n".into(), '\r' => "\\r".into(), '\t' => "\\t".into(),
                '\\' => "\\\\".into(), c if c == quote => format!("\\{c}"), c => c.to_string(),
            }).collect::<String>();
            let input = format!("table \"root\"\n  {quote}{escaped}{quote}\n");
            let document = parse(&input).unwrap();
            let EntryKind::Implicit { primary: Some(expression), .. } = &document.tables[0].entries[0].kind else { panic!() };
            prop_assert_eq!(&expression.value, &ExpressionKind::Literal(Literal::String(value.clone())));
            check_spans(&input);
        }
    }

    #[test]
    fn arbitrary_integer_ranges_keep_their_endpoints(left in any::<i64>(), right in any::<i64>()) {
        let (min, max) = (left.min(right), left.max(right));
        let input = format!("table \"root\"\n  {min}..{max}\n");
        let document = parse(&input).unwrap();
        let EntryKind::Implicit { primary: Some(expression), .. } = &document.tables[0].entries[0].kind else { panic!() };
        prop_assert_eq!(&expression.value, &ExpressionKind::Range(min, max));
    }

    #[test]
    fn truncated_fixture_never_panics(length in 0usize..500) {
        let input = include_str!("fixtures/composition.loot");
        check_spans(&input[..length.min(input.len())]);
    }
}
