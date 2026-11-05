use bevy_app::App;
use bevy_ecs::{
    message::{Message, MessageRegistry, Messages},
    system::IntoSystem,
    world::World,
};

use crate::{
    StateTreeActivity, StateTreeActivityArgs, StateTreeActivityContext, StateTreeActivityError,
    StateTreeBinding, StateTreeEffectArgs, StateTreeEffectContext, StateTreeEffectError,
    StateTreeEvent, protocol::RegistrationOrigin,
};

pub trait StateTreeExt {
    fn flush_state_tree_registrations(&mut self) -> &mut Self;

    fn add_state_tree_event<E: StateTreeEvent>(&mut self) -> &mut Self;

    fn add_state_tree_effect<M, F>(
        &mut self,
        name: impl Into<Box<str>>,
        constructor: F,
    ) -> &mut Self
    where
        M: Message,
        F: Fn(
                StateTreeEffectContext<'_>,
                &mut StateTreeEffectArgs,
            ) -> Result<M, StateTreeEffectError>
            + Send
            + Sync
            + 'static;

    fn add_state_tree_activity<A, F>(
        &mut self,
        name: impl Into<Box<str>>,
        constructor: F,
    ) -> &mut Self
    where
        A: StateTreeActivity,
        F: Fn(
                StateTreeActivityContext<'_>,
                &mut StateTreeActivityArgs,
            ) -> Result<A, StateTreeActivityError>
            + Send
            + Sync
            + 'static;

    fn add_state_tree_global_binding<B, F, S, M>(
        &mut self,
        name: impl Into<Box<str>>,
        constructor: F,
        source: S,
    ) -> &mut Self
    where
        B: StateTreeBinding,
        F: Fn(
                StateTreeActivityContext<'_>,
                &mut StateTreeActivityArgs,
            ) -> Result<B, StateTreeActivityError>
            + Send
            + Sync
            + 'static,
        S: IntoSystem<(), B::Value, M> + 'static;
}

impl StateTreeExt for World {
    fn flush_state_tree_registrations(&mut self) -> &mut Self {
        crate::binding::install_pending(self);
        self
    }

    fn add_state_tree_event<E: StateTreeEvent>(&mut self) -> &mut Self {
        crate::event::register_event::<E>(self, RegistrationOrigin::Application);
        self
    }

    fn add_state_tree_effect<M, F>(
        &mut self,
        name: impl Into<Box<str>>,
        constructor: F,
    ) -> &mut Self
    where
        M: Message,
        F: Fn(
                StateTreeEffectContext<'_>,
                &mut StateTreeEffectArgs,
            ) -> Result<M, StateTreeEffectError>
            + Send
            + Sync
            + 'static,
    {
        crate::effect::register_effect::<M, F>(
            self,
            name.into(),
            constructor,
            RegistrationOrigin::Application,
        );
        self
    }

    fn add_state_tree_activity<A, F>(
        &mut self,
        name: impl Into<Box<str>>,
        constructor: F,
    ) -> &mut Self
    where
        A: StateTreeActivity,
        F: Fn(
                StateTreeActivityContext<'_>,
                &mut StateTreeActivityArgs,
            ) -> Result<A, StateTreeActivityError>
            + Send
            + Sync
            + 'static,
    {
        crate::activity::register_activity::<A, F>(
            self,
            name.into(),
            constructor,
            RegistrationOrigin::Application,
        );
        self
    }

    fn add_state_tree_global_binding<B, F, S, M>(
        &mut self,
        name: impl Into<Box<str>>,
        constructor: F,
        source: S,
    ) -> &mut Self
    where
        B: StateTreeBinding,
        F: Fn(
                StateTreeActivityContext<'_>,
                &mut StateTreeActivityArgs,
            ) -> Result<B, StateTreeActivityError>
            + Send
            + Sync
            + 'static,
        S: IntoSystem<(), B::Value, M> + 'static,
    {
        crate::binding::register_global_binding::<B, F, S, M>(
            self,
            name.into(),
            constructor,
            source,
        );
        self
    }
}

impl StateTreeExt for App {
    fn flush_state_tree_registrations(&mut self) -> &mut Self {
        self.world_mut().flush_state_tree_registrations();
        self
    }

    fn add_state_tree_event<E: StateTreeEvent>(&mut self) -> &mut Self {
        self.world_mut().add_state_tree_event::<E>();
        self
    }

    fn add_state_tree_effect<M, F>(
        &mut self,
        name: impl Into<Box<str>>,
        constructor: F,
    ) -> &mut Self
    where
        M: Message,
        F: Fn(
                StateTreeEffectContext<'_>,
                &mut StateTreeEffectArgs,
            ) -> Result<M, StateTreeEffectError>
            + Send
            + Sync
            + 'static,
    {
        self.world_mut()
            .add_state_tree_effect::<M, F>(name, constructor);
        self
    }

    fn add_state_tree_activity<A, F>(
        &mut self,
        name: impl Into<Box<str>>,
        constructor: F,
    ) -> &mut Self
    where
        A: StateTreeActivity,
        F: Fn(
                StateTreeActivityContext<'_>,
                &mut StateTreeActivityArgs,
            ) -> Result<A, StateTreeActivityError>
            + Send
            + Sync
            + 'static,
    {
        self.world_mut()
            .add_state_tree_activity::<A, F>(name, constructor);
        self
    }

    fn add_state_tree_global_binding<B, F, S, M>(
        &mut self,
        name: impl Into<Box<str>>,
        constructor: F,
        source: S,
    ) -> &mut Self
    where
        B: StateTreeBinding,
        F: Fn(
                StateTreeActivityContext<'_>,
                &mut StateTreeActivityArgs,
            ) -> Result<B, StateTreeActivityError>
            + Send
            + Sync
            + 'static,
        S: IntoSystem<(), B::Value, M> + 'static,
    {
        self.world_mut()
            .add_state_tree_global_binding::<B, F, S, M>(name, constructor, source);
        self
    }
}

pub(crate) fn add_message<M: Message>(world: &mut World) {
    if !world.contains_resource::<Messages<M>>() {
        MessageRegistry::register_message::<M>(world);
    }
}
