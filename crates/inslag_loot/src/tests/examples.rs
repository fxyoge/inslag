#![allow(
    dead_code,
    reason = "Example entry points are not called by the system tests."
)]

use crate::{LootRegistry, SourceDocument};
use std::collections::BTreeMap;

fn finish(app: &mut bevy_app::App) {
    let mut exits = app
        .world()
        .resource::<bevy_ecs::message::Messages<bevy_app::AppExit>>()
        .get_cursor_current();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "example did not finish"
        );
        app.update();
        if let Some(exit) = exits
            .read(
                app.world()
                    .resource::<bevy_ecs::message::Messages<bevy_app::AppExit>>(),
            )
            .next()
        {
            assert_eq!(*exit, bevy_app::AppExit::Success);
            return;
        }
        std::thread::yield_now();
    }
}

mod typed_functions {
    use super::*;
    include!("../../examples/typed_functions.rs");

    #[test]
    fn startup_credits_the_existing_balance_once() {
        let mut app = app();
        app.insert_resource(Balance(5));

        finish(&mut app);
        assert_eq!(app.world().resource::<Balance>().0, 30);
        app.update();
        assert_eq!(app.world().resource::<Balance>().0, 30);
    }
}

mod encounter {
    use super::*;
    include!("../../examples/encounter.rs");
    use std::collections::BTreeSet;

    #[test]
    fn startup_spawns_the_generated_composition_once() {
        let mut app = app();
        let registry = app.world().resource::<LootRegistry>();
        let catalog = registry
            .compile([SourceDocument::new(
                "encounter.loot",
                include_str!("../../examples/assets/encounter.loot"),
            )])
            .unwrap();
        let table = catalog.table::<ActorSpec>("forest-encounter").unwrap();
        let expected = registry.generate(&table, BTreeMap::new(), 4).unwrap();
        for seed in 0..64 {
            let generated = registry.generate(&table, BTreeMap::new(), seed).unwrap();
            assert_eq!(
                generated
                    .iter()
                    .filter(|actor| actor.role == "leader")
                    .count(),
                1
            );
            let escorts: BTreeSet<_> = generated
                .iter()
                .filter(|actor| actor.role != "leader")
                .map(|actor| &actor.definition)
                .collect();
            assert_eq!(escorts.len(), 2);
            let greens = generated
                .iter()
                .filter(|actor| actor.definition == "green_slime")
                .count();
            assert!(greens == 0 || (2..=4).contains(&greens));
        }

        let mut actors = app.world_mut().query::<(&DefinitionId, &Role)>();
        assert_eq!(actors.iter(app.world()).count(), 0);
        finish(&mut app);
        let mut actual: Vec<_> = actors
            .iter(app.world())
            .map(|(definition, role)| (definition.0.clone(), role.0.clone()))
            .collect();
        let mut expected: Vec<_> = expected
            .into_iter()
            .map(|actor| (actor.definition, actor.role))
            .collect();
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected);
        app.update();
        assert_eq!(actors.iter(app.world()).count(), expected.len());
    }
}

mod contextual_rewards {
    use super::*;
    include!("../../examples/contextual_rewards.rs");

    #[test]
    fn startup_uses_context_to_spawn_items_and_credit_the_wallet() {
        for blessed in [false, true] {
            let mut app = app();
            app.insert_resource(RewardContext { blessed, gold: 25 })
                .insert_resource(Wallet(BTreeMap::from([("gold".into(), 10)])));
            let registry = app.world().resource::<LootRegistry>();
            let catalog = registry
                .compile([SourceDocument::new(
                    "contextual_rewards.loot",
                    include_str!("../../examples/assets/contextual_rewards.loot"),
                )])
                .unwrap();
            let table = catalog.table::<Reward>("rewards").unwrap();
            for seed in 0..32 {
                let generated = registry
                    .generate(
                        &table,
                        BTreeMap::from([
                            ("blessed".into(), blessed.into()),
                            ("gold".into(), 25u32.into()),
                        ]),
                        seed,
                    )
                    .unwrap();
                let items: Vec<_> = generated
                    .iter()
                    .filter_map(|reward| match reward {
                        Reward::Item(item) => Some(item),
                        _ => None,
                    })
                    .collect();
                assert_eq!(items.len(), 1);
                assert!(blessed || items[0].rarity != "unique");
                let currencies: Vec<_> = generated
                    .iter()
                    .filter_map(|reward| match reward {
                        Reward::Currency(currency) => Some(currency),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    currencies
                        .iter()
                        .find(|currency| currency.currency == "gold")
                        .unwrap()
                        .amount,
                    25
                );
                assert_eq!(
                    currencies
                        .iter()
                        .filter(|currency| currency.currency == "chaos")
                        .count(),
                    usize::from(blessed)
                );
            }

            finish(&mut app);
            let wallet = &app.world().resource::<Wallet>().0;
            assert_eq!(wallet["gold"], 35);
            assert_eq!(
                wallet.get("chaos").copied().unwrap_or_default(),
                u32::from(blessed)
            );
            let mut items = app.world_mut().query::<(&DefinitionId, &Rarity)>();
            let (definition, rarity) = items.single(app.world()).unwrap();
            assert!(["leather_tunic", "plate_vest", "silk_robe"].contains(&definition.0.as_str()));
            assert!(blessed || rarity.0 != "unique");
            app.update();
            assert_eq!(items.iter(app.world()).count(), 1);
            assert_eq!(app.world().resource::<Wallet>().0["gold"], 35);
        }
    }
}

mod weighted_sets {
    use super::*;
    include!("../../examples/weighted_sets.rs");
    use std::collections::BTreeSet;

    #[test]
    fn startup_spawns_scalar_outputs_and_weighted_sets_remain_distinct() {
        let mut app = app();
        let registry = app.world().resource::<LootRegistry>();
        let catalog = registry
            .compile([
                SourceDocument::new(
                    "weighted_sets.loot",
                    include_str!("../../examples/assets/weighted_sets.loot"),
                ),
                SourceDocument::new(
                    "compositions.loot",
                    include_str!("../../examples/assets/compositions.loot"),
                ),
            ])
            .unwrap();
        let table = catalog.table::<String>("forest").unwrap();
        let inputs = BTreeMap::from([("rare_weight".into(), 3u32.into())]);
        let mut expected = registry.generate(&table, inputs.clone(), 9).unwrap();
        let mut blood_outcomes = BTreeSet::new();
        for seed in 0..64 {
            let generated = registry.generate(&table, inputs.clone(), seed).unwrap();
            assert!((2..=4).contains(&generated.len()));
            let greens = generated
                .iter()
                .filter(|name| *name == "green_slime")
                .count();
            assert!(greens == 0 || greens == 2);
            for name in ["azure_slime", "violet_slime", "blood"] {
                assert!(generated.iter().filter(|value| *value == name).count() <= 1);
            }
            blood_outcomes.insert(generated.iter().any(|name| name == "blood"));
        }
        assert_eq!(blood_outcomes, BTreeSet::from([false, true]));

        finish(&mut app);
        let mut actors = app.world_mut().query::<&DefinitionId>();
        let mut actual: Vec<_> = actors
            .iter(app.world())
            .map(|definition| definition.0.clone())
            .collect();
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected);
        app.update();
        assert_eq!(actors.iter(app.world()).count(), expected.len());
    }
}

mod corrupted_chest {
    use super::*;
    include!("../../examples/corrupted_chest.rs");
    use std::collections::BTreeSet;

    #[test]
    fn startup_assembles_a_corrupted_item_and_rolls_cover_all_outcomes() {
        let mut app = app();
        let registry = app.world().resource::<LootRegistry>();
        let catalog = registry
            .compile([SourceDocument::new(
                "corrupted_chest.loot",
                include_str!("../../examples/assets/corrupted_chest.loot"),
            )])
            .unwrap();
        let table = catalog.table::<ChestPart>("corrupted-chest").unwrap();
        let expected = registry.generate(&table, BTreeMap::new(), 7).unwrap();
        let mut outcomes = BTreeSet::new();
        for seed in 0..256 {
            let generated = registry.generate(&table, BTreeMap::new(), seed).unwrap();
            let items: Vec<_> = generated
                .iter()
                .filter_map(|part| match part {
                    ChestPart::Item(item) => Some(item),
                    _ => None,
                })
                .collect();
            assert_eq!(items.len(), 1);
            assert!(
                ["leather_tunic", "plate_vest", "silk_robe"]
                    .contains(&items[0].definition.as_str())
            );
            assert!(generated.contains(&ChestPart::Corruption(Corruption::Mark)));
            let branch = generated.iter().find_map(|part| match part {
                ChestPart::Corruption(change) if *change != Corruption::Mark => Some(change),
                _ => None,
            });
            outcomes.insert(match branch {
                None => 0,
                Some(Corruption::Reforge { power }) => {
                    assert!((1..=20).contains(power));
                    1
                }
                Some(Corruption::WhiteSockets(amount)) => {
                    assert!((1..=items[0].sockets).contains(amount));
                    2
                }
                Some(Corruption::Implicit { .. }) => 3,
                _ => unreachable!(),
            });
        }
        assert_eq!(outcomes, BTreeSet::from([0, 1, 2, 3]));

        finish(&mut app);
        let mut items = app
            .world_mut()
            .query_filtered::<Entity, With<DefinitionId>>();
        let entity = items.single(app.world()).unwrap();
        let entity = app.world().entity(entity);
        assert!(entity.contains::<Corrupted>());
        for part in expected {
            match part {
                ChestPart::Item(item) => {
                    assert_eq!(entity.get::<DefinitionId>().unwrap().0, item.definition);
                    assert_eq!(entity.get::<Sockets>().unwrap().0, item.sockets);
                }
                ChestPart::Corruption(Corruption::Reforge { power }) => {
                    assert_eq!(entity.get::<ReforgePower>().unwrap().0, power)
                }
                ChestPart::Corruption(Corruption::WhiteSockets(amount)) => {
                    assert_eq!(entity.get::<WhiteSockets>().unwrap().0, amount)
                }
                ChestPart::Corruption(Corruption::Implicit { modifier, value }) => {
                    let implicit = entity.get::<Implicit>().unwrap();
                    assert_eq!(implicit.modifier, modifier);
                    assert_eq!(implicit.value, value);
                }
                ChestPart::Corruption(Corruption::Mark) => {}
            }
        }
        app.update();
        assert_eq!(items.iter(app.world()).count(), 1);
    }
}

mod risky_revision {
    use super::*;
    include!("../../examples/risky_revision.rs");

    #[test]
    fn startup_applies_revisions_to_existing_components() {
        let mut app = app();
        let existing = app
            .world_mut()
            .spawn((
                Quality(2.),
                Material(4.),
                Tags(BTreeSet::from(["existing".into()])),
            ))
            .id();
        let registry = app.world().resource::<LootRegistry>();
        let catalog = registry
            .compile([SourceDocument::new(
                "risky_revision.loot",
                include_str!("../../examples/assets/risky_revision.loot"),
            )])
            .unwrap();
        let table = catalog.table::<Revision>("risky-revision").unwrap();
        let expected = registry.generate(&table, BTreeMap::new(), 0).unwrap();
        let (mut quality, mut material) = (2., 4.);
        let mut tags = BTreeSet::from(["existing".into()]);
        let mut extras = Vec::new();
        for revision in expected {
            match revision {
                Revision::Attribute(attribute) => match attribute.field.as_str() {
                    "quality" => quality *= attribute.multiplier,
                    "material" => material *= attribute.multiplier,
                    _ => unreachable!(),
                },
                Revision::Tag(tag) => {
                    tags.insert(tag.0);
                }
                Revision::Item(item) => extras.push(item.0),
            }
        }
        let (mut damaged, mut cursors, mut strange, mut improved) = (0, 0, 0, 0);
        for seed in 0..512 {
            let generated = registry.generate(&table, BTreeMap::new(), seed).unwrap();
            let mut quality = 1.;
            let mut tags = BTreeSet::new();
            let mut extras = 0;
            for revision in generated {
                match revision {
                    Revision::Attribute(attribute) if attribute.field == "quality" => {
                        quality *= attribute.multiplier
                    }
                    Revision::Tag(tag) => {
                        tags.insert(tag.0);
                    }
                    Revision::Item(_) => extras += 1,
                    _ => {}
                }
            }
            assert!(tags.contains("revised"));
            damaged += usize::from(quality < 1.);
            improved += usize::from(quality > 1.);
            cursors += usize::from(tags.contains("cursor"));
            strange += usize::from(tags.contains("under_investigation"));
            assert_eq!(
                extras,
                if tags.contains("under_investigation") {
                    2
                } else {
                    0
                }
            );
        }
        assert!((310..410).contains(&damaged), "damaged={damaged}");
        assert!((20..65).contains(&cursors), "cursors={cursors}");
        assert!((1..25).contains(&strange), "strange={strange}");
        assert!((20..65).contains(&improved), "improved={improved}");

        finish(&mut app);
        let entity = app.world().entity(existing);
        assert_eq!(entity.get::<Quality>().unwrap().0, quality);
        assert_eq!(entity.get::<Material>().unwrap().0, material);
        assert_eq!(entity.get::<Tags>().unwrap().0, tags);
        let mut submissions = app.world_mut().query::<&Tags>();
        assert_eq!(submissions.iter(app.world()).count(), 2);
        assert!(
            submissions
                .iter(app.world())
                .all(|tags| tags.0.contains("revised"))
        );
        let mut items = app.world_mut().query::<&DefinitionId>();
        let actual: Vec<_> = items
            .iter(app.world())
            .map(|definition| definition.0.clone())
            .collect();
        let expected: Vec<_> = extras.iter().chain(&extras).cloned().collect();
        assert_eq!(actual, expected);
        app.update();
        assert_eq!(
            app.world().entity(existing).get::<Quality>().unwrap().0,
            quality
        );
        assert_eq!(items.iter(app.world()).count(), expected.len());
    }
}

mod asset_loading {
    include!("../../examples/asset_loading.rs");
    use std::time::Instant;

    #[test]
    fn loads_an_asset_and_spawns_its_rewards_before_exiting() {
        let mut app = app();
        app.finish();
        app.cleanup();
        let mut exits = app
            .world()
            .resource::<Messages<AppExit>>()
            .get_cursor_current();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(Instant::now() < deadline, "asset example did not exit");
            app.update();
            if let Some(exit) = exits
                .read(app.world().resource::<Messages<AppExit>>())
                .next()
            {
                assert_eq!(*exit, AppExit::Success);
                break;
            }
            std::thread::yield_now();
        }
        let mut items = app.world_mut().query::<&DefinitionId>();
        let item = items.single(app.world()).unwrap();
        assert!(["leather_tunic", "plate_vest", "silk_robe"].contains(&item.0.as_str()));
        for _ in 0..4 {
            app.update();
        }
        assert_eq!(items.iter(app.world()).count(), 1);
    }
}

mod recombinator {
    include!("../../examples/recombinator.rs");
    use bevy_ecs::system::RunSystemOnce;
    use std::collections::BTreeSet;

    fn fixture() -> App {
        let mut app = app();
        app.finish();
        app.cleanup();
        app
    }

    fn donors(app: &mut App) -> (Entity, Entity) {
        app.world_mut()
            .run_system_once(|mut commands: Commands| {
                (
                    spawn_donor(&mut commands, "leather_tunic", 1),
                    spawn_donor(&mut commands, "plate_vest", 2),
                )
            })
            .unwrap()
    }

    fn roll(
        app: &mut App,
        left: Entity,
        right: Entity,
        seed: u64,
    ) -> Result<Vec<RecombinationPart>, inslag_loot::LootError> {
        let mut cursor = app
            .world()
            .resource::<Messages<LootFinished<RecombinationPart>>>()
            .get_cursor_current();
        let request = app.world_mut().spawn(Recombine { left, right, seed }).id();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            app.update();
            if let Some(finished) = cursor
                .read(
                    app.world()
                        .resource::<Messages<LootFinished<RecombinationPart>>>(),
                )
                .next()
                .cloned()
            {
                assert_eq!(finished.owner, Some(request));
                app.update();
                assert!(app.world().get_entity(request).is_err());
                return finished.result.clone();
            }
            std::thread::yield_now();
        }
        panic!("recombination did not finish");
    }

    fn items(app: &mut App) -> Vec<Entity> {
        app.world_mut()
            .query_filtered::<Entity, With<DefinitionId>>()
            .iter(app.world())
            .collect()
    }

    #[test]
    fn startup_requests_one_recombination_and_exits_after_spawning() {
        let mut app = fixture();
        app.add_systems(Startup, setup);
        let mut exits = app
            .world()
            .resource::<Messages<AppExit>>()
            .get_cursor_current();
        let mut exited = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            app.update();
            if let Some(exit) = exits
                .read(app.world().resource::<Messages<AppExit>>())
                .next()
            {
                assert_eq!(*exit, AppExit::Success);
                exited = true;
                break;
            }
            std::thread::yield_now();
        }
        assert!(exited);
        assert_eq!(items(&mut app).len(), 3);
        assert_eq!(
            app.world_mut()
                .query::<&Recombine>()
                .iter(app.world())
                .count(),
            0
        );
        for _ in 0..4 {
            app.update();
        }
        assert_eq!(items(&mut app).len(), 3);
    }

    #[test]
    fn generates_and_spawns_compatible_items_without_changing_donors() {
        let mut app = fixture();
        let (left, right) = donors(&mut app);
        let left_before = app.world().get::<Modifiers>(left).unwrap().clone();
        let right_before = app.world().get::<Modifiers>(right).unwrap().clone();
        let (mut bases, mut counts, mut mixed) = (BTreeSet::new(), BTreeSet::new(), false);
        for seed in 0..256 {
            let generated = roll(&mut app, left, right, seed).unwrap();
            assert_eq!(app.world().get::<Modifiers>(left), Some(&left_before));
            assert_eq!(app.world().get::<Modifiers>(right), Some(&right_before));
            let selected_bases: Vec<_> = generated
                .iter()
                .filter_map(|part| match part {
                    RecombinationPart::Base(base) => Some(base),
                    _ => None,
                })
                .collect();
            assert_eq!(selected_bases.len(), 1);
            let base = selected_bases[0];
            bases.insert(base.definition.clone());
            let selected: Vec<_> = generated
                .iter()
                .filter_map(|part| match part {
                    RecombinationPart::Modifier(modifier) => Some(modifier.clone()),
                    _ => None,
                })
                .collect();
            counts.insert(
                selected
                    .iter()
                    .filter(|modifier| modifier.kind == "prefix")
                    .count(),
            );
            assert!(
                selected
                    .iter()
                    .all(|modifier| left_before.0.contains(modifier)
                        || right_before.0.contains(modifier))
            );
            assert_eq!(
                selected
                    .iter()
                    .map(|modifier| &modifier.group)
                    .collect::<BTreeSet<_>>()
                    .len(),
                selected.len()
            );
            mixed |= selected
                .iter()
                .any(|modifier| left_before.0.contains(modifier))
                && selected
                    .iter()
                    .any(|modifier| right_before.0.contains(modifier));
            let items = items(&mut app);
            assert_eq!(items.len(), 3);
            let result = *items
                .iter()
                .find(|entity| **entity != left && **entity != right)
                .unwrap();
            assert_eq!(
                app.world().get::<DefinitionId>(result).unwrap().0,
                base.definition
            );
            assert_eq!(app.world().get::<Modifiers>(result).unwrap().0, selected);
            app.world_mut().despawn(result);
        }
        assert_eq!(
            bases,
            BTreeSet::from(["leather_tunic".into(), "plate_vest".into()])
        );
        assert_eq!(counts, BTreeSet::from([0, 1, 2, 3]));
        assert!(mixed);
    }

    #[test]
    fn respects_the_selected_base_and_modifier_groups() {
        let mut app = fixture();
        let (left, right) = donors(&mut app);
        app.world_mut().get_mut::<Modifiers>(left).unwrap().0[0].restricted_to =
            Some("leather_tunic".into());
        {
            let mut modifiers = app.world_mut().get_mut::<Modifiers>(right).unwrap();
            modifiers.0[0].group = "life-1".into();
            modifiers.0[0].value = 999;
        }
        for seed in 0..64 {
            roll(&mut app, left, right, seed).unwrap();
            let result = items(&mut app)
                .into_iter()
                .find(|entity| *entity != left && *entity != right)
                .unwrap();
            let base = app.world().get::<DefinitionId>(result).unwrap();
            let modifiers = &app.world().get::<Modifiers>(result).unwrap().0;
            assert!(modifiers.iter().all(|modifier| {
                modifier
                    .restricted_to
                    .as_ref()
                    .is_none_or(|name| *name == base.0)
            }));
            assert!(
                modifiers
                    .iter()
                    .filter(|modifier| modifier.group == "life-1")
                    .count()
                    <= 1
            );
            app.world_mut().despawn(result);
        }
    }

    #[test]
    fn invalid_donors_do_not_spawn_an_item_or_change_the_surviving_donor() {
        let mut app = fixture();
        let (left, right) = donors(&mut app);
        app.world_mut().despawn(right);
        let before = app.world().get::<Modifiers>(left).unwrap().clone();
        assert!(
            roll(&mut app, left, right, 0)
                .unwrap_err()
                .message
                .contains("donor no longer exists")
        );
        assert_eq!(items(&mut app), [left]);
        assert_eq!(app.world().get::<Modifiers>(left), Some(&before));
        assert!(
            roll(&mut app, left, left, 0)
                .unwrap_err()
                .message
                .contains("distinct donors")
        );
        assert_eq!(items(&mut app), [left]);
        assert_eq!(app.world().get::<Modifiers>(left), Some(&before));
    }
}
