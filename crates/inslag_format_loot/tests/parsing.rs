use inslag_format_loot::{
    Document, EntryKind, ErrorKind, ExpressionKind, Literal, is_name, is_reserved, is_variable,
    parse,
};

fn primary(input: &str) -> ExpressionKind {
    let document = parse(&format!("table \"root\"\n  {input}\n")).unwrap();
    let EntryKind::Implicit {
        primary: Some(primary),
        ..
    } = document.tables[0].entries[0].kind.clone()
    else {
        panic!("expected implicit entry");
    };
    primary.value
}

#[test]
fn literal_types_and_expressions_are_not_runtime_values() {
    for (text, expected) in [
        ("-3", ExpressionKind::Literal(Literal::Integer(-3))),
        ("1.0", ExpressionKind::Literal(Literal::Number(1.0))),
        ("1.25e2", ExpressionKind::Literal(Literal::Number(125.0))),
        ("true", ExpressionKind::Literal(Literal::Boolean(true))),
        ("false", ExpressionKind::Literal(Literal::Boolean(false))),
        (
            "'true'",
            ExpressionKind::Literal(Literal::String("true".into())),
        ),
        (
            "forest.slime",
            ExpressionKind::Literal(Literal::String("forest.slime".into())),
        ),
        ("$input", ExpressionKind::Variable("input".into())),
        ("!$input", ExpressionKind::Not("input".into())),
        ("-4..7", ExpressionKind::Range(-4, 7)),
        (
            "-9223372036854775808..9223372036854775807",
            ExpressionKind::Range(i64::MIN, i64::MAX),
        ),
    ] {
        assert_eq!(primary(text), expected, "{text}");
    }
}

#[test]
fn strings_decode_both_quote_styles_and_every_escape() {
    assert_eq!(
        primary(r#""a;b = c\n\t\r\\\'\"🦀""#),
        ExpressionKind::Literal(Literal::String("a;b = c\n\t\r\\'\"🦀".into()))
    );
    assert_eq!(
        primary("''"),
        ExpressionKind::Literal(Literal::String(String::new()))
    );
    assert_eq!(
        primary("'a\"b'"),
        ExpressionKind::Literal(Literal::String("a\"b".into()))
    );
}

#[test]
fn guards_preserve_decoded_luau_and_source_spans_without_compiling() {
    let source = "table \"root\"\n  1 when=\"values[1] ~= 2 and math.floor(score) > 0\"\n  2 when='function( broken'\n";
    let document = parse(source).unwrap();
    let guard = document.tables[0].entries[0].condition.as_ref().unwrap();
    assert_eq!(guard.value, "values[1] ~= 2 and math.floor(score) > 0");
    assert_eq!(
        &source[guard.span.clone()],
        "\"values[1] ~= 2 and math.floor(score) > 0\""
    );
    assert_eq!(
        document.tables[0].entries[1]
            .condition
            .as_ref()
            .unwrap()
            .value,
        "function( broken"
    );
}

#[test]
fn exclusions_preserve_luau_source_and_spans_without_compiling() {
    for expression in [
        "document.color",
        "{document.color or 'ivory', 'gray'}",
        "function( broken",
        "nil",
    ] {
        let source = format!("table \"root\"\n  choose exclude=\"{expression}\"\n    ivory\n");
        let document = parse(&source).unwrap();
        let EntryKind::Choose {
            exclude: Some(exclude),
            ..
        } = &document.tables[0].entries[0].kind
        else {
            panic!()
        };
        assert_eq!(exclude.value, expression);
        assert_eq!(&source[exclude.span.clone()], format!("\"{expression}\""));
    }
    let document = parse("table \"root\"\n  choose exclude=false\n    true\n").unwrap();
    assert!(
        matches!(&document.tables[0].entries[0].kind, EntryKind::Choose { exclude: Some(value), .. } if value.value == "false")
    );
}

#[test]
fn exclusions_are_choose_only_and_cannot_be_duplicated() {
    for entry in ["1", "all", "nothing", "@actor", "#other"] {
        let source = format!("table \"root\"\n  {entry} exclude=\"current\"\n");
        let error = parse(&source).unwrap_err();
        assert_eq!(error.kind, ErrorKind::ChooseAttributeOutsideChoose);
        assert_eq!(&source[error.span], "exclude");
    }
    let error = parse("table \"root\"\n  choose exclude=1 exclude=2\n    1\n").unwrap_err();
    assert_eq!(error.kind, ErrorKind::DuplicateAttribute("exclude".into()));
    assert!(
        matches!(parse("table \"root\" using=actor exclude=1\n").unwrap_err().kind, ErrorKind::ReservedAttribute(name) if name == "exclude")
    );
}

#[test]
fn comments_blank_lines_line_endings_and_final_newlines_do_not_change_structure() {
    for input in [
        "table \"root\"\n  1\n",
        "table \"root\"\r\n  1\r\n",
        "table \"root\"\n  1",
        "; intro\n\ntable \"root\" ; header\n ; ignored indent\n  1; trailing\n    ; ignored\n",
    ] {
        let document: Document = input.parse().unwrap();
        assert_eq!(document.tables.len(), 1);
        assert_eq!(document.tables[0].entries.len(), 1);
    }
}

#[test]
fn omission_is_distinct_from_an_explicit_default() {
    let document =
        parse("table \"root\"\n  choose\n    1\n  choose rolls=1 distinct=false\n    2 weight=1\n")
            .unwrap();
    let entries = &document.tables[0].entries;
    let EntryKind::Choose {
        entries: children,
        rolls,
        distinct,
        exclude,
    } = &entries[0].kind
    else {
        panic!()
    };
    assert!(
        rolls.is_none() && distinct.is_none() && exclude.is_none() && children[0].weight.is_none()
    );
    let EntryKind::Choose {
        entries: children,
        rolls,
        distinct,
        ..
    } = &entries[1].kind
    else {
        panic!()
    };
    assert!(rolls.is_some() && distinct.is_some() && children[0].weight.is_some());
}

#[test]
fn arguments_preserve_author_order_and_exact_byte_spans() {
    let input =
        "table \"root\" using=actor z=3 a=1\n  @actor label=\"🦀\" z=2 a=$input as=result\n";
    let document = parse(input).unwrap();
    let table = &document.tables[0];
    assert_eq!(&input[table.name.span.clone()], "\"root\"");
    assert_eq!(&input[table.using.as_ref().unwrap().span.clone()], "actor");
    assert_eq!(
        table
            .defaults
            .iter()
            .map(|a| a.name.value.as_str())
            .collect::<Vec<_>>(),
        ["z", "a"]
    );
    let entry = &table.entries[0];
    assert_eq!(
        &input[entry.span.clone()],
        "@actor label=\"🦀\" z=2 a=$input as=result"
    );
    let EntryKind::Call {
        function,
        arguments,
    } = &entry.kind
    else {
        panic!()
    };
    assert_eq!(&input[function.span.clone()], "actor");
    assert_eq!(
        arguments
            .iter()
            .map(|a| a.name.value.as_str())
            .collect::<Vec<_>>(),
        ["label", "z", "a"]
    );
    assert_eq!(&input[arguments[0].name.span.clone()], "label");
    assert_eq!(&input[arguments[0].value.span.clone()], "\"🦀\"");
    assert_eq!(&input[arguments[2].span.clone()], "a=$input");
    assert_eq!(
        &input[entry.binding.as_ref().unwrap().span.clone()],
        "result"
    );
}

#[test]
fn syntax_does_not_link_type_check_or_evaluate() {
    for input in [
        "table \"root\" using=unknown\n  unknown\n",
        "table \"root\"\n  @unknown name=$missing\n  #missing\n",
        "table \"root\"\n  #root\n",
        "table \"root\"\ntable \"root\"\n",
        "table \"root\"\n  $input\n  as=empty\n",
        "table \"root\"\n  choose rolls=-1\n    1 weight=false when=42\n",
    ] {
        parse(input).unwrap();
    }
}

#[test]
fn composition_fixture_preserves_nested_groups_and_references() {
    let input = include_str!("fixtures/composition.loot");
    let document = parse(input).unwrap();
    assert_eq!(document.tables.len(), 3);
    let table = &document.tables[0];
    assert_eq!(table.entries[0].binding.as_ref().unwrap().value, "leader");
    let EntryKind::Choose { entries, .. } = &table.entries[1].kind else {
        panic!()
    };
    assert!(
        matches!(&entries[0].kind, EntryKind::Table { table, .. } if table.value == "green-escort")
    );
    assert!(matches!(&entries[2].kind, EntryKind::All(children) if children.len() == 2));
    assert_eq!(table.span.end, input.find("\n\ntable").unwrap());
}

#[test]
fn grammar_vocabulary_is_available_without_a_runtime() {
    assert!(is_variable("_actor-2"));
    assert!(!is_variable("actor.name"));
    assert!(is_name("forest.actor-2"));
    for name in ["", "2actor", "forest.", ".forest", "forest..actor", "🦀"] {
        assert!(!is_name(name), "{name}");
    }
    for name in [
        "table", "choose", "all", "nothing", "using", "as", "weight", "when", "rolls", "distinct",
        "exclude",
    ] {
        assert!(is_reserved(name));
    }
    assert!(!is_reserved("actor"));
}

#[test]
fn malformed_files_have_structured_errors() {
    let cases = [
        ("", ErrorKind::EmptyDocument),
        ("; comments only", ErrorKind::EmptyDocument),
        ("table", ErrorKind::ExpectedTable),
        ("table root", ErrorKind::ExpectedQuotedTableName),
        ("table 'root'", ErrorKind::ExpectedQuotedTableName),
        (" table \"root\"", ErrorKind::ExpectedTable),
        ("table \"bad name\"", ErrorKind::InvalidTableName),
        (
            "table \"root\" using=bad!",
            ErrorKind::InvalidDefaultFunction,
        ),
        ("table \"root\"\n\t1", ErrorKind::InvalidWhitespace),
        ("table \"root\"\n  1\u{a0}2", ErrorKind::InvalidWhitespace),
        (
            "table \"root\"\n  \"unclosed",
            ErrorKind::UnterminatedString,
        ),
        (
            "table \"root\"\n  \"x\"tail",
            ErrorKind::UnexpectedAfterString,
        ),
        ("table \"root\"\n  \"a\\q\"", ErrorKind::InvalidEscape),
        ("table \"root\"\n  @actor value=", ErrorKind::MissingValue),
        (
            "table \"root\"\n  @actor value",
            ErrorKind::ExpectedAttribute,
        ),
        (
            "table \"root\"\n  @actor 2value=x",
            ErrorKind::InvalidAttribute,
        ),
        (
            "table \"root\"\n  @actor value=1 value=2",
            ErrorKind::DuplicateAttribute("value".into()),
        ),
        (
            "table \"root\"\n  @actor using=x",
            ErrorKind::ReservedAttribute("using".into()),
        ),
        ("table \"root\"\n  choose", ErrorKind::MissingChildren),
        ("table \"root\"\n  2\n    3", ErrorKind::UnexpectedChildren),
        (
            "table \"root\"\n  all\n    1\n   2",
            ErrorKind::InconsistentIndentation,
        ),
        ("table \"root\"\n  3..1", ErrorKind::ReversedRange),
        ("table \"root\"\n  1.5..2.5", ErrorKind::InvalidRange),
        ("table \"root\"\n  NaN", ErrorKind::NonFiniteNumber),
        ("table \"root\"\n  -inf", ErrorKind::NonFiniteNumber),
        ("table \"root\"\n  1e999", ErrorKind::NonFiniteNumber),
        (
            "table \"root\"\n  @identity(value=1)",
            ErrorKind::InvalidReference,
        ),
        (
            "table \"root\"\n  @actor value=$item.mods",
            ErrorKind::InvalidVariable,
        ),
        (
            "table \"root\"\n  @actor value=!$2",
            ErrorKind::InvalidVariable,
        ),
        ("table \"root\"\n  x as=2", ErrorKind::InvalidBinding),
        ("table \"root\"\n  x:y", ErrorKind::InvalidValue),
        (
            "table \"root\"\n  choose from=$pool\n    1",
            ErrorKind::UnknownGroupAttribute,
        ),
        (
            "table \"root\"\n  1 weight=2",
            ErrorKind::WeightOutsideChoose,
        ),
        (
            "table \"root\"\n  nothing count=2",
            ErrorKind::UnexpectedArguments,
        ),
        (
            "table \"root\"\n  all rolls=2\n    1",
            ErrorKind::ChooseAttributeOutsideChoose,
        ),
        (
            "table \"root\"\n  choose distinct=maybe\n    1",
            ErrorKind::InvalidDistinct,
        ),
        (
            "table \"root\" variant=rare",
            ErrorKind::HeaderArgumentsWithoutUsing,
        ),
    ];
    for (input, expected) in cases {
        let error = parse(input).unwrap_err();
        assert_eq!(error.kind, expected, "{input:?}: {error}");
        assert!(input.get(error.span).is_some());
    }
}

#[test]
fn diagnostics_point_to_the_offending_token_not_the_start_of_the_line() {
    for (input, offending) in [
        (
            "table \"root\"\n  @actor name=\"🦀\" value=1 value=2",
            "value",
        ),
        ("table \"root\"\n  @actor name=\"🦀\" value=$a.b", "$a.b"),
        ("table \"root\"\n  choose distinct=maybe\n    1", "maybe"),
        ("table \"root\"\n  @actor name=\"a\\q\"", "\\q"),
        ("table \"root\"\n  @actor name=\"x\"tail", "t"),
        ("table \"root\"\n  @actor name=\"x\"\\q", "\\"),
        ("table \"root\"\n  1\u{a0}2", "\u{a0}"),
    ] {
        let error = parse(input).unwrap_err();
        assert_eq!(&input[error.span.clone()], offending, "{error}");
        assert!(error.span.start >= input.rfind(offending).unwrap());
    }
}

#[test]
fn nesting_is_bounded_at_the_format_boundary() {
    fn nested(groups: usize) -> String {
        let mut input = "table \"root\"\n".to_owned();
        for depth in 1..=groups {
            input.push_str(&format!("{}all\n", " ".repeat(depth)));
        }
        input.push_str(&format!("{}1\n", " ".repeat(groups + 1)));
        input
    }
    parse(&nested(63)).unwrap();
    let error = parse(&nested(64)).unwrap_err();
    assert_eq!(error.kind, ErrorKind::NestingLimit);
}
