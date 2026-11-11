#![expect(dead_code)]
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/support/model.rs"
));
use bevy_ecs::{
    query::{QueryData, QueryFilter},
    system::SystemParam,
};

type HealthQuery<'w, 's> = Query<'w, 's, &'static Health>;

fn alias(query: HealthQuery) -> bool {
    query.is_empty()
}

fn optional(query: Query<Option<&Health>>) -> bool {
    query.is_empty()
}

fn mixed_optional(query: Query<(&Health, Option<&Shield>)>) -> bool {
    query.is_empty()
}

fn mutable(query: Query<&mut Health>) -> bool {
    query.is_empty()
}

fn has(query: Query<(&Health, Has<Shield>)>) -> bool {
    query.is_empty()
}

#[derive(QueryData)]
struct Data {
    health: &'static Health,
}

fn custom_data(query: Query<Data>) -> bool {
    query.is_empty()
}

#[derive(QueryFilter)]
struct Filter {
    _shield: With<Shield>,
}

fn custom_filter(query: Query<&Health, Filter>) -> bool {
    query.is_empty()
}

fn generic_filter<F: QueryFilter>(query: Query<&Health, F>) -> bool {
    query.is_empty()
}

fn filter_only(query: Query<(), With<Health>>) -> bool {
    query.is_empty()
}

fn entity_only(query: Query<Entity, With<Health>>) -> bool {
    query.is_empty()
}

fn param_set(mut queries: ParamSet<(Query<&Health>, Query<&Shield>)>) -> bool {
    queries.p0().is_empty()
}

#[derive(SystemParam)]
struct Params<'w, 's> {
    query: Query<'w, 's, &'static Health>,
}

fn wrapper(params: Params) -> bool {
    params.query.is_empty()
}

fn lens(mut query: Query<&Health>) -> bool {
    query.as_query_lens().query().is_empty()
}

mod unrelated {
    struct Query<T>(T);
    impl<T> Query<T> {
        fn is_empty(&self) -> bool {
            false
        }
    }
    fn check(query: Query<&super::Health>) -> bool {
        query.is_empty()
    }
}

fn main() {}
