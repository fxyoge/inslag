use super::{GeneratorAppExt, LootFunctionContext, typed_signature};
use crate::{LootArgs, LootReturn};
use bevy_app::App;
use bevy_ecs::system::{ReadOnlySystemParam, SystemParamItem};

/// A native generator callable from loot sources.
pub trait LootFunction<M>: private::Register<M> {}

impl<M, F: private::Register<M>> LootFunction<M> for F {}

pub(super) fn register<M>(app: &mut App, name: &str, generator: impl LootFunction<M>) {
    private::Register::register(generator, app, name);
}

mod private {
    use super::*;

    pub trait Register<M>: Send + Sync + 'static {
        fn register(self, app: &mut App, name: &str);
    }

    pub struct Plain;
    pub struct Context;
    pub struct Params;
    pub struct ArgsParams;
    pub struct ContextParams;
    pub struct ContextArgsParams;

    impl<R, F> Register<fn() -> R> for F
    where
        R: LootReturn,
        F: Fn() -> R + Send + Sync + 'static,
    {
        fn register(self, app: &mut App, name: &str) {
            app.register_generator(name, typed_signature::<(), R>(), move |_, _| {
                self().into_values()
            });
        }
    }

    impl<A, R, F> Register<(Plain, fn(A) -> R)> for F
    where
        A: LootArgs + 'static,
        R: LootReturn,
        F: Fn(A) -> R + Send + Sync + 'static,
    {
        fn register(self, app: &mut App, name: &str) {
            app.register_generator(name, typed_signature::<A, R>(), move |_, args| {
                self(A::from_arguments(args)?).into_values()
            });
        }
    }

    impl<R, F> Register<fn(&mut LootFunctionContext) -> R> for F
    where
        R: LootReturn,
        F: Fn(&mut LootFunctionContext) -> R + Send + Sync + 'static,
    {
        fn register(self, app: &mut App, name: &str) {
            app.register_generator(name, typed_signature::<(), R>(), move |context, _| {
                self(context).into_values()
            });
        }
    }

    impl<A, R, F> Register<(Context, fn(A) -> R)> for F
    where
        A: LootArgs + 'static,
        R: LootReturn,
        F: Fn(&mut LootFunctionContext, A) -> R + Send + Sync + 'static,
    {
        fn register(self, app: &mut App, name: &str) {
            app.register_generator(name, typed_signature::<A, R>(), move |context, args| {
                self(context, A::from_arguments(args)?).into_values()
            });
        }
    }

    macro_rules! impl_params {
        ($($param:ident: $value:ident),+) => {
            impl<R, F, $($param),+> Register<(Params, fn($($param),+) -> R)> for F
            where
                R: LootReturn,
                $($param: ReadOnlySystemParam + 'static,)+
                F: Fn($($param),+) -> R
                    + for<'w, 's> Fn($(SystemParamItem<'w, 's, $param>),+) -> R
                    + Send + Sync + 'static,
            {
                fn register(self, app: &mut App, name: &str) {
                    fn call<R, $($param),+>(function: &impl Fn($($param),+) -> R, ($($value,)+): ($($param,)+)) -> R {
                        function($($value),+)
                    }
                    app.register_contextual::<($($param,)+), _>(name, typed_signature::<(), R>(), move |_, _, ($($value,)+)| {
                        call(&self, ($($value,)+)).into_values()
                    });
                }
            }

            impl<A, R, F, $($param),+> Register<(ArgsParams, fn(A, $($param),+) -> R)> for F
            where
                A: LootArgs + 'static,
                R: LootReturn,
                $($param: ReadOnlySystemParam + 'static,)+
                F: Fn(A, $($param),+) -> R
                    + for<'w, 's> Fn(A, $(SystemParamItem<'w, 's, $param>),+) -> R
                    + Send + Sync + 'static,
            {
                fn register(self, app: &mut App, name: &str) {
                    fn call<A, R, $($param),+>(function: &impl Fn(A, $($param),+) -> R, args: A, ($($value,)+): ($($param,)+)) -> R {
                        function(args, $($value),+)
                    }
                    app.register_contextual::<($($param,)+), _>(name, typed_signature::<A, R>(), move |_, args, ($($value,)+)| {
                        call(&self, A::from_arguments(args)?, ($($value,)+)).into_values()
                    });
                }
            }

            impl<R, F, $($param),+> Register<(ContextParams, fn($($param),+) -> R)> for F
            where
                R: LootReturn,
                $($param: ReadOnlySystemParam + 'static,)+
                F: Fn(&mut LootFunctionContext, $($param),+) -> R
                    + for<'w, 's> Fn(&mut LootFunctionContext, $(SystemParamItem<'w, 's, $param>),+) -> R
                    + Send + Sync + 'static,
            {
                fn register(self, app: &mut App, name: &str) {
                    fn call<R, $($param),+>(function: &impl Fn(&mut LootFunctionContext, $($param),+) -> R, context: &mut LootFunctionContext, ($($value,)+): ($($param,)+)) -> R {
                        function(context, $($value),+)
                    }
                    app.register_contextual::<($($param,)+), _>(name, typed_signature::<(), R>(), move |context, _, ($($value,)+)| {
                        call(&self, context, ($($value,)+)).into_values()
                    });
                }
            }

            impl<A, R, F, $($param),+> Register<(ContextArgsParams, fn(A, $($param),+) -> R)> for F
            where
                A: LootArgs + 'static,
                R: LootReturn,
                $($param: ReadOnlySystemParam + 'static,)+
                F: Fn(&mut LootFunctionContext, A, $($param),+) -> R
                    + for<'w, 's> Fn(&mut LootFunctionContext, A, $(SystemParamItem<'w, 's, $param>),+) -> R
                    + Send + Sync + 'static,
            {
                fn register(self, app: &mut App, name: &str) {
                    fn call<A, R, $($param),+>(function: &impl Fn(&mut LootFunctionContext, A, $($param),+) -> R, context: &mut LootFunctionContext, args: A, ($($value,)+): ($($param,)+)) -> R {
                        function(context, args, $($value),+)
                    }
                    app.register_contextual::<($($param,)+), _>(name, typed_signature::<A, R>(), move |context, args, ($($value,)+)| {
                        call(&self, context, A::from_arguments(args)?, ($($value,)+)).into_values()
                    });
                }
            }
        };
    }

    impl_params!(P0: p0);
    impl_params!(P0: p0, P1: p1);
    impl_params!(P0: p0, P1: p1, P2: p2);
    impl_params!(P0: p0, P1: p1, P2: p2, P3: p3);
    impl_params!(P0: p0, P1: p1, P2: p2, P3: p3, P4: p4);
    impl_params!(P0: p0, P1: p1, P2: p2, P3: p3, P4: p4, P5: p5);
    impl_params!(P0: p0, P1: p1, P2: p2, P3: p3, P4: p4, P5: p5, P6: p6);
    impl_params!(P0: p0, P1: p1, P2: p2, P3: p3, P4: p4, P5: p5, P6: p6, P7: p7);
}
