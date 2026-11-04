use crate::*;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, PartialEq, LootOutput)]
struct Base {
    name: String,
}
#[derive(Clone, Debug, PartialEq, LootOutput)]
struct Affix {
    value: i64,
}

#[test]
fn bags_are_ordinary_vectors_of_consumer_defined_enums() {
    #[derive(Clone, Debug, PartialEq, LootOutput)]
    enum Part {
        Base(Base),
        Affix(Affix),
        Text(String),
    }
    let mut bag: Vec<Part> = vec![
        Base {
            name: "vest".into(),
        }
        .into(),
    ];
    bag.push(Affix { value: 20 }.into());
    bag.push(Affix { value: 30 }.into());
    bag.push("other".to_string().into());
    assert_eq!(bag.len(), 4);
    assert!(matches!(&bag[0], Part::Base(Base { name }) if name == "vest"));
    assert_eq!(bag[3], Part::Text("other".into()));
    assert_eq!(bag.clone(), bag);
}

#[test]
fn data_types_are_nominal_and_payload_equality_is_structural() {
    let left = Data::new(Base {
        name: "vest".into(),
    });
    let right = Data::new(Base {
        name: "vest".into(),
    });
    assert_eq!(left, right);
    assert_ne!(left, Data::new(Affix { value: 1 }));
    assert!(ValueType::data::<Base>().accepts(&Value::Data(left.clone())));
    assert!(!ValueType::data::<Affix>().accepts(&Value::Data(left.clone())));
    assert!(left.clone().downcast::<Affix>().is_err());
    assert_eq!(left.downcast::<Base>().unwrap().name, "vest");
}

#[test]
fn aliases_share_immutable_generated_data_without_extracting_it_from_the_bag() {
    #[derive(Clone, Debug, PartialEq, LootOutput)]
    enum Part {
        Base(Base),
        Affix(Affix),
    }
    #[derive(LootArgs)]
    struct Related {
        base: Arc<Base>,
    }
    let mut app = bevy_app::App::new();
    app.add_loot_function("base", || Base {
        name: "vest".into(),
    })
    .add_loot_function("related", |args: Related| {
        assert!(Arc::strong_count(&args.base) > 1);
        Affix {
            value: args.base.name.len() as i64,
        }
    });
    let registry = app.world().resource::<LootRegistry>();
    let catalog = registry
        .compile([SourceDocument::new(
            "data.loot",
            "table \"root\"\n  @base as=base\n  @related base=$base\n",
        )])
        .unwrap();
    let generated = registry
        .generate(&catalog.table::<Part>("root").unwrap(), BTreeMap::new(), 0)
        .unwrap();
    assert_eq!(generated.len(), 2);
    assert_eq!(
        generated,
        [
            Part::Base(Base {
                name: "vest".into()
            }),
            Part::Affix(Affix { value: 4 })
        ]
    );
}

#[test]
fn output_schema_validates_every_generated_element_not_just_the_first() {
    let mut signatures = FunctionSignatures::new();
    signatures
        .register(
            "mixed",
            FunctionSignature::new().emits(ValueType::data::<Base>()),
        )
        .unwrap();
    let catalog = Arc::new(
        Catalog::compile(
            [SourceDocument::new(
                "bad.loot",
                "table \"root\"\n  @mixed\n",
            )],
            &signatures,
        )
        .unwrap(),
    );
    let mut session = Session::new(catalog, "root", BTreeMap::new(), 0).unwrap();
    assert!(matches!(session.advance().unwrap(), Step::Call(_)));
    let outputs = vec![
        Value::data(Base {
            name: "vest".into(),
        }),
        Value::data(Affix { value: 4 }),
    ];
    assert!(
        session
            .resume(Ok(outputs))
            .unwrap_err()
            .message
            .contains("generator emitted")
    );
}
