use crate::*;
use std::{collections::BTreeMap, sync::Arc};

fn run(text: &str, inputs: BTreeMap<String, Value>, seed: u64) -> (Vec<Value>, Vec<FunctionCall>) {
    let mut session = Session::new(compile(text), "root", inputs, seed)
        .unwrap()
        .with_limits(Limits {
            max_results: 100_000,
            ..Limits::default()
        });
    let mut calls = Vec::new();
    loop {
        match session.advance().unwrap() {
            Step::Call(call) => {
                let generated = match call.function.as_str() {
                    "sample" => vec![Reference::new("sample", calls.len() as u64 + 1).into()],
                    "identity" => vec![call.arguments.get("value").unwrap().clone()],
                    "none" => Vec::new(),
                    _ => unreachable!(),
                };
                calls.push(call);
                session.resume(Ok(generated)).unwrap();
            }
            Step::Finished(generated) => return (generated, calls),
            Step::Waiting => panic!("test forgot to complete a generator"),
        }
    }
}

fn compile(text: &str) -> Arc<Catalog> {
    Arc::new(Catalog::compile([SourceDocument::new("test.loot", text)], &signatures()).unwrap())
}
fn error(text: &str) -> LootError {
    Catalog::compile([SourceDocument::new("bad.loot", text)], &signatures()).unwrap_err()
}

fn signatures() -> FunctionSignatures {
    let mut signatures = FunctionSignatures::new();
    signatures
        .register(
            "sample",
            FunctionSignature::new()
                .required("name", ValueType::String)
                .optional("variant", ValueType::String, "normal")
                .primary("name")
                .emits(ValueType::reference("sample")),
        )
        .unwrap();
    signatures
        .register("none", FunctionSignature::new())
        .unwrap();
    signatures
        .register(
            "identity",
            FunctionSignature::new()
                .required("value", ValueType::Any)
                .primary("value")
                .emits(ValueType::Any),
        )
        .unwrap();
    signatures
}

#[test]
fn scalar_values_ranges_comments_and_escaping() {
    let (values, calls) = run(
        "table \"root\" ; comment\r\n  -3\r\n  1.25\r\n  true\r\n  'a;b = c'\r\n  \"line\\nquote\\\"\"\r\n  -4..-4\r\n",
        BTreeMap::new(),
        0,
    );
    assert_eq!(
        values,
        vec![
            (-3i64).into(),
            1.25.into(),
            true.into(),
            "a;b = c".into(),
            "line\nquote\"".into(),
            (-4i64).into()
        ]
    );
    assert!(calls.is_empty());
}

#[test]
fn primary_named_overrides_and_explicit_calls_have_lexical_defaults() {
    let (_, calls) = run(
        "table \"root\" using=sample variant=rare\n  alpha\n  name=beta\n  gamma variant=unique\n  @sample name=delta\n",
        BTreeMap::new(),
        0,
    );
    assert_eq!(
        calls
            .iter()
            .map(|call| call.arguments.get("name").unwrap().clone())
            .collect::<Vec<_>>(),
        vec![
            "alpha".into(),
            "beta".into(),
            "gamma".into(),
            "delta".into()
        ]
    );
    assert_eq!(
        calls
            .iter()
            .map(|call| call.arguments.get("variant").unwrap().clone())
            .collect::<Vec<_>>(),
        vec![
            "rare".into(),
            "rare".into(),
            "unique".into(),
            "normal".into()
        ]
    );
}

#[test]
fn referenced_tables_do_not_inherit_defaults_or_caller_variables() {
    let (values, calls) = run(
        "table \"root\" using=sample variant=rare\n  #other\ntable \"other\"\n  plain_value\n  @sample name=alpha\n",
        BTreeMap::new(),
        0,
    );
    assert_eq!(values[0], Value::from("plain_value"));
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].arguments.get("variant"),
        Some(&Value::from("normal"))
    );
    assert!(
        error("table \"root\"\n  #other\ntable \"other\"\n  @identity value=$context\n")
            .message
            .contains("requires input")
    );
}

#[test]
fn unused_aliases_never_remove_or_duplicate_generated_results() {
    let plain = "table \"root\"\n  #pool\ntable \"pool\"\n  choose\n    @sample name=alpha\n    @sample name=beta\n";
    let aliased = "table \"root\"\n  #pool as=chosen\ntable \"pool\"\n  choose\n    @sample name=alpha\n    @sample name=beta\n";
    for seed in 0..64 {
        let (left, left_calls) = run(plain, BTreeMap::new(), seed);
        let (right, right_calls) = run(aliased, BTreeMap::new(), seed);
        assert_eq!(left, right);
        assert_eq!(left_calls[0].seed, right_calls[0].seed);
        assert_eq!(
            left_calls[0].arguments.get("name"),
            right_calls[0].arguments.get("name")
        );
    }
}

#[test]
fn aliases_are_immutable_inputs_to_later_generators_without_a_return_statement() {
    let (values, calls) = run(
        "table \"root\"\n  @sample name=alpha as=chosen\n  #related selected=$chosen\ntable \"related\"\n  @identity value=$selected\n",
        BTreeMap::new(),
        0,
    );
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1].arguments.get("value"),
        Some(&Value::from(Reference::new("sample", 1)))
    );
    assert_eq!(values, vec![Value::from(Reference::new("sample", 1)); 2]);
}

#[test]
fn standalone_variables_are_not_fallthrough_returns() {
    for source in [
        "table \"root\"\n  1 as=x\n  $x\n",
        "table \"root\"\n  $input\n",
    ] {
        assert!(
            error(source)
                .message
                .contains("standalone variables are not returns")
        );
    }
}

#[test]
fn repeated_choices_isolate_branch_bindings() {
    let (values, calls) = run(
        "table \"root\"\n  choose rolls=3\n    all\n      @sample name=alpha as=local\n      @identity value=$local\n",
        BTreeMap::new(),
        0,
    );
    assert_eq!(calls.len(), 6);
    assert_eq!(values.len(), 6);
    for pair in values.chunks(2) {
        assert_eq!(pair[0], pair[1]);
    }
    let (values, _) = run(
        "table \"root\"\n  choose rolls=2 distinct=true\n    all\n      1 as=x\n    all\n      2 as=x\n",
        BTreeMap::new(),
        0,
    );
    assert_eq!(values.len(), 2);
    assert!(values.contains(&1i64.into()) && values.contains(&2i64.into()));
}

#[test]
fn table_inputs_are_explicit_and_inferred_from_argument_references() {
    let source =
        "table \"root\"\n  #child input=$provided\ntable \"child\"\n  @identity value=$input\n";
    let catalog = compile(source);
    assert_eq!(
        catalog.required_inputs("root").unwrap().collect::<Vec<_>>(),
        ["provided"]
    );
    assert_eq!(
        catalog
            .required_inputs("child")
            .unwrap()
            .collect::<Vec<_>>(),
        ["input"]
    );
    assert_eq!(
        run(
            source,
            BTreeMap::from([("provided".into(), 42i64.into())]),
            0
        )
        .0,
        [42i64.into()]
    );
}

#[test]
fn guards_zero_weights_and_zero_rolls_only_change_the_bag() {
    let source = "table \"root\"\n  choose rolls=20\n    1 when=\"enabled\"\n    2 when=\"not enabled\"\n    3 weight=0\n  choose rolls=0\n    @sample name=never\n";
    for (enabled, expected) in [(true, 1), (false, 2)] {
        let (values, calls) = run(
            source,
            BTreeMap::from([("enabled".into(), enabled.into())]),
            0,
        );
        assert_eq!(values, vec![Value::Integer(expected); 20]);
        assert!(calls.is_empty());
    }
    let (values, calls) = run(
        "table \"root\"\n  choose\n    @sample name=never when=false\n  nothing\n",
        BTreeMap::new(),
        0,
    );
    assert!(values.is_empty() && calls.is_empty());
}

#[test]
fn exclusions_accept_scalars_sequences_and_sparse_optional_values() {
    for expression in ["1", "1.0", "{1, 1, nil}", "{nil, 1, nil}", "{[100] = 1}"] {
        let source = format!(
            "table \"root\"\n  choose rolls=50 exclude=\"{expression}\"\n    1 weight=999\n    2\n"
        );
        assert_eq!(run(&source, BTreeMap::new(), 0).0, vec![2i64.into(); 50]);
    }
    for (expression, expected) in [
        ("'ivory'", "gray"),
        ("{'ivory', 'gray'}", "pink"),
        ("{nil, 'ivory', nil, 'gray'}", "pink"),
    ] {
        let source = format!(
            "table \"root\"\n  choose exclude=\"{expression}\"\n    ivory weight=999\n    gray\n{}",
            if expected == "pink" { "    pink\n" } else { "" }
        );
        for seed in 0..16 {
            assert_eq!(
                run(&source, BTreeMap::new(), seed).0,
                [Value::from(expected)]
            );
        }
    }
    assert_eq!(
        run(
            "table \"root\"\n  choose exclude=false\n    false weight=999\n    true\n",
            BTreeMap::new(),
            0
        )
        .0,
        [true.into()]
    );
    for expression in ["nil", "{}", "{nil, nil}"] {
        let source = format!(
            "table \"root\"\n  choose rolls=2 distinct=true exclude=\"{expression}\"\n    1\n    2\n"
        );
        let values = run(&source, BTreeMap::new(), 0).0;
        assert_eq!(values.len(), 2);
        assert!(values.contains(&1i64.into()) && values.contains(&2i64.into()));
    }
    assert_eq!(run("table \"root\"\n  choose rolls=20 exclude=\"{'1', false}\"\n    '1'\n    false\n    1\n", BTreeMap::new(), 0).0, vec![1i64.into(); 20]);
}

#[test]
fn exclusions_filter_primary_arguments_without_executing_generators() {
    for candidate in ["1", "value=1", "@identity value=1"] {
        let source = format!(
            "table \"root\" using=identity\n  choose rolls=20 exclude=\"{{1, 2}}\"\n    {candidate} weight=999\n    2\n    3\n"
        );
        let (values, calls) = run(&source, BTreeMap::new(), 0);
        assert_eq!(values, vec![3i64.into(); 20]);
        assert_eq!(calls.len(), 20);
        assert!(
            calls
                .iter()
                .all(|call| call.arguments.get("value") == Some(&3i64.into()))
        );
    }
    let (_, calls) = run(
        "table \"root\" using=sample\n  choose exclude=\"'alpha'\"\n    alpha\n    beta\n",
        BTreeMap::new(),
        0,
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].arguments.get("name"), Some(&"beta".into()));
}

#[test]
fn exclusions_infer_inputs_and_honor_lexical_scope() {
    let source = "table \"root\"\n  1 as=previous\n  #pool exclusions=$unavailable selected=$candidate prior=$previous\ntable \"pool\" using=identity\n  choose rolls=30 exclude=\"{prior, exclusions[1]}\"\n    $selected\n    1\n    2\n";
    let catalog = compile(source);
    assert_eq!(
        catalog.required_inputs("root").unwrap().collect::<Vec<_>>(),
        ["candidate", "unavailable"]
    );
    let values = run(
        source,
        BTreeMap::from([
            ("candidate".into(), 3i64.into()),
            ("unavailable".into(), vec![2i64.into()].into()),
        ]),
        0,
    )
    .0;
    assert_eq!(values[0], 1i64.into());
    assert_eq!(values[1..], vec![3i64.into(); 30]);
    for source in [
        "table \"root\"\n  choose exclude=\"later\"\n    1\n  2 as=later\n",
        "table \"root\"\n  all\n    2 as=inner\n  choose exclude=\"inner\"\n    1\n",
        "table \"root\"\n  choose exclude=\"inner\"\n    1 as=inner\n",
    ] {
        assert!(error(source).message.contains("not available"));
    }
}

#[test]
fn exclusions_preserve_weighting_random_streams_and_remaining_guards() {
    let old = "table \"root\" using=identity\n  choose rolls=100\n    1 weight=999 when=\"current ~= 1\"\n    2 weight=3 when=\"enabled and current ~= 2\"\n    3 when=\"current ~= 3\"\n";
    let new = "table \"root\" using=identity\n  choose rolls=100 exclude=\"current\"\n    1 weight=999\n    2 weight=3 when=\"enabled\"\n    3\n";
    for enabled in [true, false] {
        for seed in 0..32 {
            let inputs = BTreeMap::from([
                ("current".into(), 1.0.into()),
                ("enabled".into(), enabled.into()),
            ]);
            let (old_values, old_calls) = run(old, inputs.clone(), seed);
            let (new_values, new_calls) = run(new, inputs, seed);
            assert_eq!(old_values, new_values);
            assert_eq!(
                old_calls.iter().map(|call| call.seed).collect::<Vec<_>>(),
                new_calls.iter().map(|call| call.seed).collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn exclusion_lists_can_come_from_borrowed_context_views() {
    let source = "table \"root\"\n  choose rolls=20 exclude=\"unavailable\"\n    1\n    2\n    3\n";
    let inputs = BTreeMap::from([(
        "unavailable".into(),
        vec![1i64.into(), Value::Nil, 2i64.into()].into(),
    )]);
    assert_eq!(run(source, inputs.clone(), 0).0, vec![3i64.into(); 20]);
    let mut session = Session::new(compile(source), "root", inputs, 0).unwrap();
    let reader = |_: &Reference,
                  _: &[inslag_luau::LuauValue]|
     -> Result<inslag_luau::ReadOnlyValue, String> { unreachable!() };
    let Step::Finished(values) = session.advance_with(Some(&reader)).unwrap() else {
        panic!()
    };
    assert_eq!(values, vec![3i64.into(); 20]);
}

#[test]
fn exclusion_expressions_are_evaluated_once_per_invocation_and_see_live_references() {
    use inslag_luau::{LuauValue, ReadOnlyValue};
    use std::cell::Cell;
    let source = "table \"root\"\n  #pool document=$document\n  #pool document=$document\ntable \"pool\"\n  choose rolls=20 exclude=\"document.current or 'ivory'\"\n    ivory\n    pink\n";
    let mut session = Session::new(
        compile(source),
        "root",
        BTreeMap::from([("document".into(), Reference::new("document", 1).into())]),
        0,
    )
    .unwrap();
    let reads = Cell::new(0);
    let reader = |_: &Reference, path: &[LuauValue]| -> Result<ReadOnlyValue, String> {
        Ok(match path {
            [] => ReadOnlyValue::View,
            [LuauValue::String(field)] if field == "current" => {
                let count = reads.get();
                reads.set(count + 1);
                ReadOnlyValue::Value(if count == 0 {
                    LuauValue::Nil
                } else {
                    LuauValue::String("pink".into())
                })
            }
            _ => unreachable!(),
        })
    };
    let Step::Finished(values) = session.advance_with(Some(&reader)).unwrap() else {
        panic!()
    };
    assert_eq!(reads.get(), 2);
    assert_eq!(values[..20], vec![Value::from("pink"); 20]);
    assert_eq!(values[20..], vec![Value::from("ivory"); 20]);
}

#[test]
fn exclusions_preserve_empty_choices_zero_rolls_and_distinct_exhaustion() {
    for source in [
        "table \"root\"\n  choose exclude=\"{1, 2}\"\n    1\n    2\n",
        "table \"root\"\n  choose rolls=0 exclude=1\n    2\n",
        "table \"root\"\n  choose exclude=1\n    1 as=skipped\n    2 weight=0\n",
    ] {
        assert!(run(source, BTreeMap::new(), 0).0.is_empty());
    }
    let source = "table \"root\"\n  choose rolls=2 distinct=true exclude=1\n    1\n    2\n";
    assert!(
        Session::new(compile(source), "root", BTreeMap::new(), 0)
            .unwrap()
            .advance()
            .unwrap_err()
            .message
            .contains("exhausted")
    );
    let source = "table \"root\"\n  choose rolls=2 distinct=true exclude=1\n    1\n    2\n    2\n";
    assert_eq!(
        run(source, BTreeMap::new(), 0).0,
        [2i64.into(), 2i64.into()]
    );
}

#[test]
fn exclusion_failures_have_source_locations_and_reject_unsupported_shapes() {
    assert!(
        error("table \"root\"\n  choose exclude=\"invalid +\"\n    1\n")
            .message
            .contains("invalid exclude expression")
    );
    for expression in [
        "{name = 1}",
        "{[0] = 1}",
        "{[-1] = 1}",
        "{[1.5] = 1}",
        "{1, {2}}",
        "function() end",
        "math.random()",
        "(function() while true do end end)()",
        "(function() values[1] = 2 return values end)()",
    ] {
        let source = format!("table \"root\"\n  choose exclude=\"{expression}\"\n    1\n");
        let catalog = compile(&source);
        let inputs = catalog
            .required_inputs("root")
            .unwrap()
            .map(|name| (name.to_owned(), vec![1i64.into()].into()))
            .collect();
        let error = Session::new(catalog, "root", inputs, 0)
            .unwrap()
            .advance()
            .unwrap_err();
        assert!(
            error.message.contains("exclude expression failed"),
            "{expression}: {error}"
        );
        assert_eq!(error.location.line, 2);
        assert_eq!(error.table_path, ["root"]);
    }
    for candidate in ["1..3", "#other", "@none", "all\n      1", "choose\n      1"] {
        let source = format!("table \"root\"\n  choose exclude=1\n    {candidate}\n");
        assert!(
            error(&source)
                .message
                .contains("requires scalar candidates")
        );
    }
    let source = "table \"root\" using=identity\n  choose exclude=1\n    $candidate\n";
    let error = Session::new(
        compile(source),
        "root",
        BTreeMap::from([("candidate".into(), vec![1i64.into()].into())]),
        0,
    )
    .unwrap()
    .advance()
    .unwrap_err();
    assert!(error.message.contains("requires scalar candidate values"));
    assert_eq!(error.location.line, 3);
}

#[test]
fn exclusion_inputs_reject_records_and_nonfinite_numbers_in_both_context_modes() {
    let reader = |_: &Reference,
                  _: &[inslag_luau::LuauValue]|
     -> Result<inslag_luau::ReadOnlyValue, String> { unreachable!() };
    for expression in [
        "{name = 1}",
        "{1, name = 2}",
        "{1, {2}}",
        "1 / 0",
        "0 / 0",
        "{1, 1 / 0}",
        "unavailable",
    ] {
        let source = format!("table \"root\"\n  choose exclude=\"{expression}\"\n    1\n");
        for contextual in [true, false] {
            let inputs = if expression == "unavailable" {
                BTreeMap::from([(
                    "unavailable".into(),
                    Value::Record(Arc::new(BTreeMap::from([("named".into(), 1i64.into())]))),
                )])
            } else {
                BTreeMap::new()
            };
            let mut session = Session::new(compile(&source), "root", inputs, 0).unwrap();
            let error = session
                .advance_with(if contextual { Some(&reader) } else { None })
                .unwrap_err();
            assert!(error.message.contains("exclude"), "{expression}: {error}");
            assert_eq!(error.location.line, 2);
            assert_eq!(error.table_path, ["root"]);
        }
    }
}

#[test]
fn luau_guards_read_inputs_aliases_and_builtins_before_weighted_sampling() {
    let source = "table \"root\"\n  2 as=minimum\n  choose rolls=200\n    10 weight=999 when=\"values[1] > minimum\"\n    20 when=\"math.floor(score) == minimum and not enabled\"\n";
    let catalog = compile(source);
    assert_eq!(
        catalog.required_inputs("root").unwrap().collect::<Vec<_>>(),
        ["enabled", "score", "values"]
    );
    let inputs = BTreeMap::from([
        ("values".into(), Value::from(vec![1i64.into()])),
        ("score".into(), 2.9.into()),
        ("enabled".into(), false.into()),
    ]);
    let (values, calls) = run(source, inputs.clone(), 32);
    assert!(calls.is_empty());
    assert_eq!(values[0], 2i64.into());
    assert_eq!(values[1..], vec![Value::Integer(20); 200]);
    assert_eq!(values, run(source, inputs, 32).0);
}

#[test]
fn luau_guards_use_truthiness_and_support_functions_and_records() {
    let source = "table \"root\"\n  1 when=\"(function() for _, v in rows do if v.stage ~= 'complete' then return false end end return true end)()\"\n  2 when=0\n  3 when=\"nil\"\n  4 when=false\n";
    let row = inslag_luau::LuauValue::Record(BTreeMap::from([(
        "stage".into(),
        inslag_luau::LuauValue::String("complete".into()),
    )]));
    let inputs = BTreeMap::from([(
        "rows".into(),
        Value::from(inslag_luau::LuauValue::Sequence(vec![row])),
    )]);
    assert_eq!(run(source, inputs, 0).0, [1i64.into(), 2i64.into()]);
}

#[test]
fn luau_guard_failures_have_source_locations_and_do_not_allow_mutation_or_randomness() {
    for (source, input) in [
        ("rows[1] = 2; return true", true),
        ("return math.random() > 0", false),
        ("while true do end", false),
        ("leaked = 1; return true", false),
    ] {
        let text = format!("table \"root\"\n  1 when=\"(function() {source} end)()\"\n");
        let catalog = compile(&text);
        let inputs = catalog
            .required_inputs("root")
            .unwrap()
            .map(|name| {
                (
                    name.to_owned(),
                    if input {
                        Value::from(vec![1i64.into()])
                    } else {
                        1i64.into()
                    },
                )
            })
            .collect();
        let error = Session::new(catalog, "root", inputs, 0)
            .unwrap()
            .advance()
            .unwrap_err();
        let error = error.to_string();
        assert!(error.contains("test.loot:2:"), "{error}");
        assert!(error.contains("when expression failed"), "{error}");
    }
    for source in [
        "table \"root\"\n  1 when=\"later > 0\"\n  2 as=later\n",
        "table \"root\"\n  all\n    2 as=inner\n  1 when=\"inner > 0\"\n",
    ] {
        assert!(error(source).message.contains("not available"));
    }
    assert!(
        error("table \"root\"\n  1 when=$enabled\n")
            .message
            .contains("invalid when expression")
    );
}

#[test]
fn opaque_guard_dependencies_fail_but_unreferenced_native_data_does_not() {
    let catalog =
        compile("table \"root\"\n  @sample name=alpha as=actor\n  1 when=\"actor ~= nil\"\n");
    let mut session = Session::new(catalog, "root", BTreeMap::new(), 0).unwrap();
    assert!(matches!(session.advance().unwrap(), Step::Call(_)));
    session
        .resume(Ok(vec![Reference::new("sample", 1).into()]))
        .unwrap();
    assert!(session.advance().unwrap_err().message.contains("opaque"));
    let (_, calls) = run(
        "table \"root\"\n  @sample name=alpha as=actor\n  1 when=\"true\"\n",
        BTreeMap::new(),
        0,
    );
    assert_eq!(calls.len(), 1);
}

#[test]
fn distinct_selects_entries_not_output_values_and_resets_for_each_invocation() {
    let (values, _) = run(
        "table \"root\"\n  #pool\n  #pool\ntable \"pool\"\n  choose rolls=2 distinct=true\n    same\n    same\n",
        BTreeMap::new(),
        0,
    );
    assert_eq!(values, vec![Value::from("same"); 4]);
}

#[test]
fn weighted_sampling_matches_authored_odds() {
    let (values, _) = run(
        "table \"root\"\n  choose rolls=20000\n    0 weight=1\n    1 weight=3\n",
        BTreeMap::new(),
        123,
    );
    let ones = values
        .iter()
        .filter(|value| **value == Value::Integer(1))
        .count();
    assert!((14_700..15_300).contains(&ones), "observed {ones}/20000");
}

#[test]
fn sampled_arguments_cover_signed_ranges_and_extreme_endpoints() {
    let (_, calls) = run(
        "table \"root\" using=identity\n  choose rolls=100\n    -3..3\n",
        BTreeMap::new(),
        0,
    );
    let values: Vec<_> = calls
        .iter()
        .map(|call| call.arguments.get("value").unwrap())
        .collect();
    assert!(values.contains(&&Value::Integer(-3)) && values.contains(&&Value::Integer(3)));
    assert!(
        values
            .iter()
            .all(|value| matches!(value, Value::Integer(-3..=3)))
    );
    assert!(matches!(
        run(
            "table \"root\"\n  -9223372036854775808..9223372036854775807\n",
            BTreeMap::new(),
            0
        )
        .0
        .as_slice(),
        [Value::Integer(_)]
    ));
    assert_eq!(
        run(
            "table \"root\"\n  -9223372036854775808..-9223372036854775808\n",
            BTreeMap::new(),
            0
        )
        .0,
        [Value::Integer(i64::MIN)]
    );
}

#[test]
fn waiting_does_not_reissue_a_generator_or_change_its_random_stream() {
    let source = compile("table \"root\"\n  @identity value=1..100\n");
    let mut session = Session::new(source, "root", BTreeMap::new(), 4).unwrap();
    let Step::Call(call) = session.advance().unwrap() else {
        panic!()
    };
    for _ in 0..8 {
        assert!(matches!(session.advance().unwrap(), Step::Waiting));
    }
    let value = call.arguments.get("value").unwrap().clone();
    session.resume(Ok(vec![value.clone()])).unwrap();
    let Step::Finished(generated) = session.advance().unwrap() else {
        panic!()
    };
    assert_eq!(generated, [value]);
    assert_eq!(session.trace().len(), 1);
}

#[test]
fn traces_and_cross_file_errors_keep_source_locations() {
    let mut session = Session::new(
        compile("table \"root\"\n  choose\n    @identity value=4\n"),
        "root",
        BTreeMap::new(),
        0,
    )
    .unwrap();
    let Step::Call(call) = session.advance().unwrap() else {
        panic!()
    };
    assert_eq!(call.location.line, 3);
    assert!(
        matches!(&session.trace()[0], TraceEvent::Selected { location, child: 0 } if location.line == 2)
    );
    let catalog = Catalog::compile(
        [
            SourceDocument::new("a.loot", "table \"root\"\n  #other\n"),
            SourceDocument::new("b.loot", "table \"other\"\n  42\n"),
        ],
        &signatures(),
    )
    .unwrap();
    let mut session = Session::new(Arc::new(catalog), "root", BTreeMap::new(), 0).unwrap();
    let Step::Finished(generated) = session.advance().unwrap() else {
        panic!()
    };
    assert_eq!(generated, [42i64.into()]);
    let error = Catalog::compile(
        [
            SourceDocument::new("a.loot", "table \"root\""),
            SourceDocument::new("b.loot", "table \"root\""),
        ],
        &signatures(),
    )
    .unwrap_err();
    assert_eq!(error.location.source.as_ref(), "b.loot");
    assert!(error.message.contains("a.loot:1:1"));
}

#[test]
fn format_errors_attach_source_names_and_unicode_columns() {
    let error = error("table \"root\"\r\n  @identity value=\"🦀\" value=2");
    assert_eq!(error.location.source.as_ref(), "bad.loot");
    assert_eq!(error.location.line, 2);
    assert_eq!(error.location.column, 23);
    assert!(error.message.contains("duplicate attribute"));
}

#[test]
fn invalid_runtime_metadata_is_rejected_during_lowering() {
    let cases = [
        ("table \"root\"\n  as=x", "entry needs a value"),
        ("table \"root\"\n  choose rolls=-1\n    1", "negative"),
        ("table \"root\"\n  choose\n    1 weight=-1..5", "negative"),
        (
            "table \"root\"\n  choose rolls=false\n    1",
            "expects Integer",
        ),
        (
            "table \"root\"\n  1 when=\"invalid +\"",
            "invalid when expression",
        ),
    ];
    for (source, expected) in cases {
        let error = error(source);
        assert!(
            error.message.contains(expected),
            "{source:?}: expected {expected:?}, got {error}"
        );
        assert_eq!(error.location.source.as_ref(), "bad.loot");
    }
}

#[test]
fn unknown_generators_arguments_and_table_inputs_fail_at_link_time() {
    let cases = [
        ("table \"root\" using=missing", "unknown default function"),
        ("table \"root\"\n  @missing", "unknown function"),
        ("table \"root\"\n  @sample", "missing argument"),
        (
            "table \"root\"\n  @sample name=x typo=y",
            "unknown argument",
        ),
        ("table \"root\"\n  @sample name=1", "expects String"),
        ("table \"root\"\n  @none as=x", "emits no value"),
        ("table \"root\"\n  #missing", "unknown table"),
        (
            "table \"root\"\n  #child\ntable \"child\"\n  @identity value=$input",
            "requires input",
        ),
        (
            "table \"root\"\n  #child typo=1\ntable \"child\"",
            "has no input",
        ),
        ("table \"root\" variant=rare", "require using"),
        (
            "table \"root\" using=identity\n  1 value=2",
            "also supplied by name",
        ),
        ("table \"root\" using=none\n  1", "no primary parameter"),
    ];
    for (source, expected) in cases {
        assert!(
            error(source).message.contains(expected),
            "{source:?}: {expected}"
        );
    }
}

#[test]
fn aliases_cannot_be_forward_referenced_shadowed_or_escape_their_block() {
    for source in [
        "table \"root\"\n  @identity value=$x\n  1 as=x\n",
        "table \"root\"\n  1 as=x\n  2 as=x\n",
        "table \"root\"\n  all\n    1 as=x\n  @identity value=$x\n",
        "table \"root\"\n  choose\n    1 as=x\n    @identity value=$x\n",
    ] {
        assert!(error(source).message.contains("binding"));
    }
}

#[test]
fn cycles_and_nested_unknown_references_fail_without_entering_the_evaluator() {
    assert!(
        error("table \"root\"\n  #other\ntable \"other\"\n  choose\n    #root weight=0\n")
            .message
            .contains("cycle")
    );
    let error = error("table \"a\"\n  #b\ntable \"b\"\n  #missing\n");
    assert!(error.message.contains("unknown table"));
    assert_eq!(error.location.line, 4);
}

#[test]
fn context_types_missing_inputs_and_extra_inputs_are_checked() {
    let catalog = compile("table \"root\"\n  @sample name=$input\n");
    assert!(
        Session::new(catalog.clone(), "root", BTreeMap::new(), 0)
            .unwrap_err()
            .message
            .contains("requires input")
    );
    let mut session = Session::new(
        catalog.clone(),
        "root",
        BTreeMap::from([("input".into(), 1i64.into())]),
        0,
    )
    .unwrap();
    assert!(
        session
            .advance()
            .unwrap_err()
            .message
            .contains("expects String")
    );
    assert!(
        Session::new(
            catalog,
            "root",
            BTreeMap::from([("input".into(), "ok".into()), ("extra".into(), true.into())]),
            0
        )
        .unwrap_err()
        .message
        .contains("has no input")
    );
}

#[test]
fn native_generators_can_emit_many_results_but_single_aliases_remain_exact() {
    let mut session = Session::new(
        compile("table \"root\"\n  @identity value=1\n"),
        "root",
        BTreeMap::new(),
        0,
    )
    .unwrap();
    assert!(matches!(session.advance().unwrap(), Step::Call(_)));
    let bag = vec![1i64.into(), 2i64.into()];
    session.resume(Ok(bag)).unwrap();
    let Step::Finished(generated) = session.advance().unwrap() else {
        panic!()
    };
    assert_eq!(generated, [1i64.into(), 2i64.into()]);
    for (source, expected) in [
        ("table \"root\"\n  nothing as=x\n", "got 0"),
        ("table \"root\"\n  all as=x\n    1\n    2\n", "got 2"),
        ("table \"root\"\n  1 as=x when=false\n", "skipped"),
    ] {
        let mut session = Session::new(compile(source), "root", BTreeMap::new(), 0).unwrap();
        assert!(session.advance().unwrap_err().message.contains(expected));
    }
}

#[test]
fn incorrect_native_outputs_and_failures_are_terminal() {
    let mut session = Session::new(
        compile("table \"root\"\n  @sample name=alpha\n"),
        "root",
        BTreeMap::new(),
        0,
    )
    .unwrap();
    assert!(
        session
            .resume(Ok(Vec::new()))
            .unwrap_err()
            .message
            .contains("not awaiting")
    );
    assert!(matches!(session.advance().unwrap(), Step::Call(_)));
    let error = session
        .resume(Ok(vec![Reference::new("wrong", 1).into()]))
        .unwrap_err();
    assert!(error.message.contains("generator emitted"));
    assert_eq!(session.advance().unwrap_err(), error);
    let mut session = Session::new(
        compile("table \"root\"\n  @identity value=1\n"),
        "root",
        BTreeMap::new(),
        0,
    )
    .unwrap();
    assert!(matches!(session.advance().unwrap(), Step::Call(_)));
    let error = session.resume(Err("context rejected".into())).unwrap_err();
    assert!(error.message.contains("context rejected"));
    assert_eq!(error.table_path, ["root"]);
}

#[test]
fn selection_failures_and_resource_limits_have_explicit_diagnostics() {
    for (source, expected) in [
        (
            "table \"root\"\n  choose rolls=2 distinct=true\n    1\n",
            "exhausted",
        ),
        (
            "table \"root\"\n  choose\n    1 weight=9223372036854775807\n    2 weight=9223372036854775807\n    3 weight=2\n",
            "overflow",
        ),
    ] {
        let mut session = Session::new(compile(source), "root", BTreeMap::new(), 0).unwrap();
        assert!(session.advance().unwrap_err().message.contains(expected));
    }
    for (source, limits, expected) in [
        (
            "table \"root\"\n  1\n  2",
            Limits {
                max_steps: 1,
                ..Limits::default()
            },
            "step limit",
        ),
        (
            "table \"root\"\n  all\n    1",
            Limits {
                max_depth: 1,
                ..Limits::default()
            },
            "depth limit",
        ),
        (
            "table \"root\"\n  1 as=one\n  2",
            Limits {
                max_results: 1,
                ..Limits::default()
            },
            "result limit",
        ),
        (
            "table \"root\"\n  @identity value=1",
            Limits {
                max_calls: 0,
                ..Limits::default()
            },
            "call limit",
        ),
    ] {
        let mut session = Session::new(compile(source), "root", BTreeMap::new(), 0)
            .unwrap()
            .with_limits(limits);
        assert!(session.advance().unwrap_err().message.contains(expected));
    }
}

#[test]
fn signature_validation_protects_language_keywords_and_defaults() {
    for name in [
        "table", "choose", "all", "nothing", "weight", "when", "as", "using", "rolls", "distinct",
        "exclude",
    ] {
        assert!(
            FunctionSignatures::new()
                .register(
                    "test",
                    FunctionSignature::new().required(name, ValueType::Any)
                )
                .unwrap_err()
                .contains("reserved")
        );
    }
    assert!(
        FunctionSignatures::new()
            .register("test", FunctionSignature::new().primary("missing"))
            .unwrap_err()
            .contains("not declared")
    );
    assert!(
        FunctionSignatures::new()
            .register(
                "test",
                FunctionSignature::new().optional("x", ValueType::Integer, "wrong")
            )
            .unwrap_err()
            .contains("invalid default")
    );
    assert!(
        signatures()
            .register("identity", FunctionSignature::new())
            .unwrap_err()
            .contains("already registered")
    );
}

#[test]
fn scalar_nan_and_infinity_are_rejected_at_every_boundary() {
    for value in [
        Value::Number(f64::NAN),
        Value::Number(f64::INFINITY),
        vec![Value::Number(f64::NEG_INFINITY)].into(),
    ] {
        assert!(
            Session::new(
                compile("table \"root\"\n  @identity value=$input\n"),
                "root",
                BTreeMap::from([("input".into(), value)]),
                0
            )
            .unwrap_err()
            .message
            .contains("finite")
        );
    }
    assert!(
        FunctionSignatures::new()
            .register(
                "bad",
                FunctionSignature::new().optional("value", ValueType::Any, f64::NAN)
            )
            .unwrap_err()
            .contains("invalid default")
    );
    let mut session = Session::new(
        compile("table \"root\"\n  @identity value=1\n"),
        "root",
        BTreeMap::new(),
        0,
    )
    .unwrap();
    assert!(matches!(session.advance().unwrap(), Step::Call(_)));
    assert!(
        session
            .resume(Ok(vec![f64::NAN.into()]))
            .unwrap_err()
            .message
            .contains("generator emitted")
    );
}

#[test]
fn list_context_has_a_type_and_does_not_become_an_implicit_sampling_source() {
    let mut functions = FunctionSignatures::new();
    functions
        .register(
            "inspect",
            FunctionSignature::new()
                .required("items", ValueType::list(ValueType::reference("item")))
                .emits(ValueType::Integer),
        )
        .unwrap();
    let catalog = Arc::new(
        Catalog::compile(
            [SourceDocument::new(
                "lists.loot",
                "table \"root\"\n  @inspect items=$donors as=amount\n",
            )],
            &functions,
        )
        .unwrap(),
    );
    let donors: Value = vec![
        Reference::new("item", 1).into(),
        Reference::new("item", 2).into(),
    ]
    .into();
    let mut session = Session::new(
        catalog.clone(),
        "root",
        BTreeMap::from([("donors".into(), donors)]),
        0,
    )
    .unwrap();
    let Step::Call(mut call) = session.advance().unwrap() else {
        panic!()
    };
    assert_eq!(
        call.arguments.take::<Vec<Reference>>("items").unwrap(),
        [Reference::new("item", 1), Reference::new("item", 2)]
    );
    session.resume(Ok(vec![2i64.into()])).unwrap();
    let Step::Finished(generated) = session.advance().unwrap() else {
        panic!()
    };
    assert_eq!(generated, [2i64.into()]);
    let mut session = Session::new(
        catalog,
        "root",
        BTreeMap::from([(
            "donors".into(),
            vec![Reference::new("affix", 1).into()].into(),
        )]),
        0,
    )
    .unwrap();
    assert!(
        session
            .advance()
            .unwrap_err()
            .message
            .contains("expects List")
    );
}

#[test]
fn context_can_supply_primary_arguments_weights_and_roll_counts() {
    let (values, calls) = run(
        "table \"root\" using=identity\n  choose rolls=$draws\n    $payload weight=$enabled_weight\n    ignored weight=0\n",
        BTreeMap::from([
            ("draws".into(), 3i64.into()),
            ("payload".into(), Reference::new("custom", 42).into()),
            ("enabled_weight".into(), 5i64.into()),
        ]),
        0,
    );
    assert_eq!(calls.len(), 3);
    assert_eq!(values, vec![Value::from(Reference::new("custom", 42)); 3]);
}

#[test]
fn reference_depth_is_bounded() {
    let mut source = String::new();
    for index in 0..260 {
        source.push_str(&format!("table \"t{index}\"\n  #t{}\n", index + 1));
    }
    source.push_str("table \"t260\"\n  1\n");
    assert!(error(&source).message.contains("reference depth exceeds"));
}
