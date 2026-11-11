use bevy_ecs::{
    change_detection::Tick,
    prelude::*,
    query::{QueryFilter, ReadOnlyQueryData},
};

macro_rules! examples {
    ($($name:ident),+ $(,)?) => {
        $(#[test]
        fn $name() {
            dylint_testing::ui_test_example(env!("CARGO_PKG_NAME"), stringify!($name));
        })+
    };
}

examples!(
    membership,
    filters,
    aliases,
    actual_reads,
    contracts,
    unsupported,
    macros,
    registered,
    lifetimes,
    attributes,
    generic_changes,
);

#[derive(Component)]
struct Health;
#[derive(Component)]
struct Shield;
#[derive(Component)]
struct Hidden;

#[derive(Component)]
#[component(storage = "SparseSet")]
struct Sparse;

fn equivalent<D: ReadOnlyQueryData, F: QueryFilter, G: QueryFilter>(
    world: &mut World,
    entities: &[Entity],
    last: Tick,
) {
    let original = QueryState::<D, F>::new(world);
    let replacement = QueryState::<(), G>::new(world);
    let now = world.change_tick();
    assert_eq!(
        original.is_empty(world, last, now),
        replacement.is_empty(world, last, now)
    );
    for &entity in entities {
        assert_eq!(
            original.contains(entity, world, last, now),
            replacement.contains(entity, world, last, now)
        );
    }
}

#[test]
fn replacement_preserves_membership() {
    let mut world = World::new();
    let empty = world.spawn_empty().id();
    let health = world.spawn(Health).id();
    let both = world.spawn((Health, Shield)).id();
    let hidden = world.spawn((Health, Shield, Hidden)).id();
    let gone = world.spawn(Health).id();
    world.despawn(gone);
    let entities = [empty, health, both, hidden, gone];
    for _ in 0..2 {
        equivalent::<&Health, (), With<Health>>(&mut world, &entities, Tick::new(0));
        equivalent::<
            (Entity, &Health, (&Shield, ())),
            Without<Hidden>,
            (With<Health>, With<Shield>, Without<Hidden>),
        >(&mut world, &entities, Tick::new(0));
        equivalent::<
            &Health,
            Or<(With<Shield>, Without<Hidden>)>,
            (With<Health>, Or<(With<Shield>, Without<Hidden>)>),
        >(&mut world, &entities, Tick::new(0));
        world.entity_mut(both).remove::<Shield>();
        world.entity_mut(empty).insert((Health, Shield));
    }
}

#[test]
fn replacement_preserves_change_filters() {
    let mut world = World::new();
    let entity = world.spawn((Health, Shield)).id();
    let last = world.change_tick();
    world.increment_change_tick();
    for changed in [false, true] {
        if changed {
            world.get_mut::<Shield>(entity).unwrap().set_changed();
        }
        equivalent::<&Health, Changed<Shield>, (With<Health>, Changed<Shield>)>(
            &mut world,
            &[entity],
            last,
        );
        equivalent::<(&Health, &Shield), Added<Health>, (With<Health>, With<Shield>, Added<Health>)>(
            &mut world,
            &[entity],
            last,
        );
    }
}

#[test]
fn replacement_preserves_sparse_and_disabled_membership() {
    use bevy_ecs::entity_disabling::Disabled;

    let mut world = World::new();
    let visible = world.spawn((Health, Sparse)).id();
    let disabled = world.spawn((Health, Sparse, Disabled)).id();
    let entities = [visible, disabled];
    equivalent::<&Sparse, (), With<Sparse>>(&mut world, &entities, Tick::new(0));
    equivalent::<(&Sparse, &Disabled), (), (With<Sparse>, With<Disabled>)>(
        &mut world,
        &entities,
        Tick::new(0),
    );
}

#[test]
fn replacement_removes_read_conflict() {
    let mut world = World::new();
    let original = QueryState::<&Health>::new(&mut world);
    let replacement = QueryState::<(), With<Health>>::new(&mut world);
    let writer = QueryState::<&mut Health>::new(&mut world);
    assert!(
        !original
            .component_access()
            .is_compatible(writer.component_access())
    );
    assert!(
        replacement
            .component_access()
            .is_compatible(writer.component_access())
    );
}

#[test]
fn change_filters_still_require_read_access() {
    let mut world = World::new();
    let changed = QueryState::<(), (With<Health>, Changed<Health>)>::new(&mut world);
    let added = QueryState::<(), (With<Health>, Added<Health>)>::new(&mut world);
    let writer = QueryState::<&mut Health>::new(&mut world);
    assert!(
        !changed
            .component_access()
            .is_compatible(writer.component_access())
    );
    assert!(
        !added
            .component_access()
            .is_compatible(writer.component_access())
    );
}
