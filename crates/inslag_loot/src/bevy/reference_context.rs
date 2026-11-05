use super::*;
use crate::Reference;
use inslag_luau::{LuauValue, ReadOnlyValue};

/// A live reference view cannot retain ECS borrows after its evaluation returns.
pub trait LootReferenceContext<M>: private::Register<M> {}
impl<M, F: private::Register<M>> LootReferenceContext<M> for F {}

pub(super) fn register<M>(app: &mut App, name: &str, reader: impl LootReferenceContext<M>) {
    private::Register::register(reader, app, name);
}

fn register_context<P, F>(app: &mut App, name: &str, reader: F)
where
    P: ReadOnlySystemParam + 'static,
    F: for<'w, 's> Fn(
            &Reference,
            &[LuauValue],
            SystemParamItem<'w, 's, P>,
        ) -> Result<ReadOnlyValue, String>
        + Send
        + Sync
        + 'static,
{
    assert!(
        app.world().contains_resource::<Runtime>(),
        "install LootPlugin before reference contexts"
    );
    assert!(
        !SystemState::<P>::new(app.world_mut()).meta().has_deferred(),
        "loot contexts cannot use deferred world mutations"
    );
    assert!(
        !name.is_empty()
            && app
                .world_mut()
                .resource_mut::<ContextRegistrations>()
                .0
                .insert(name.into()),
        "loot context names must be nonempty and unique"
    );
    let name = name.to_owned();
    app.add_systems(
        PostUpdate,
        (move |mut calls: MessageReader<ContextCall>,
               mut runtime: ResMut<Runtime>,
               registry: Res<LootRegistry>,
               mut native: MessageWriter<NativeCall>,
               mut finished: MessageWriter<NativeFinished>,
               mut params: ParamSet<(Result<P, SystemParamValidationError>,)>| {
            for call in calls.read() {
                if call.name != name
                    || !runtime.runs.get(&call.id).is_some_and(|run| {
                        run.awaiting_context
                            && run.context_epoch == call.epoch
                            && run.context.as_deref() == Some(&name)
                    })
                {
                    continue;
                }
                let mut run = runtime.runs.remove(&call.id).unwrap();
                run.awaiting_context = false;
                let params = std::cell::RefCell::new(&mut params);
                let resolve = |reference: &Reference, path: &[LuauValue]| {
                    let mut params = params.borrow_mut();
                    let params = params
                        .p0()
                        .map_err(|error| format!("invalid context {name:?}: {error}"))?;
                    reader(reference, path, params)
                };
                advance_with(
                    call.id,
                    run,
                    &mut runtime,
                    &registry,
                    &mut native,
                    &mut finished,
                    Some(&resolve),
                );
            }
        })
        .in_set(LootSet::Generate)
        .run_if(on_message::<ContextCall>),
    );
}

mod private {
    use super::*;
    pub trait Register<M>: Send + Sync + 'static {
        fn register(self, app: &mut App, name: &str);
    }

    impl<F> Register<fn(&Reference, &[LuauValue]) -> Result<ReadOnlyValue, String>> for F
    where
        F: Fn(&Reference, &[LuauValue]) -> Result<ReadOnlyValue, String> + Send + Sync + 'static,
    {
        fn register(self, app: &mut App, name: &str) {
            register_context::<(), _>(app, name, move |reference, path, ()| self(reference, path));
        }
    }

    macro_rules! params {
        ($($param:ident : $value:ident),+) => {
            impl<F, $($param),+> Register<fn(&Reference, &[LuauValue], $($param),+) -> Result<ReadOnlyValue, String>> for F
            where $($param: ReadOnlySystemParam + 'static,)+
                F: Fn(&Reference, &[LuauValue], $($param),+) -> Result<ReadOnlyValue, String>
                    + for<'w, 's> Fn(&Reference, &[LuauValue], $(SystemParamItem<'w, 's, $param>),+) -> Result<ReadOnlyValue, String>
                    + Send + Sync + 'static,
            {
                fn register(self, app: &mut App, name: &str) {
                    register_context::<($($param,)+), _>(app, name, move |reference, path, ($($value,)+)| self(reference, path, $($value),+));
                }
            }
        };
    }
    params!(A: a);
    params!(A: a, B: b);
    params!(A: a, B: b, C: c);
    params!(A: a, B: b, C: c, D: d);
}
