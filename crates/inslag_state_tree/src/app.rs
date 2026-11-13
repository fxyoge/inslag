use bevy_app::App;
use bevy_ecs::{
    message::{Message, MessageRegistry, Messages},
    system::IntoSystem,
    world::World,
};

use crate::{
    Activity, StateTreeActivity, StateTreeBinding, StateTreeEvent, StateTreeInput,
    protocol::RegistrationOrigin,
};

pub trait StateTreeExt {
    fn flush_state_tree_registrations(&mut self) -> &mut Self;
    fn add_state_tree_event<E: StateTreeEvent>(&mut self) -> &mut Self;
    fn add_state_tree_effect<M: Message + StateTreeInput>(
        &mut self,
        name: impl Into<Box<str>>,
    ) -> &mut Self;
    fn add_state_tree_activity<A: StateTreeActivity + StateTreeInput>(
        &mut self,
        activity: Activity<A>,
    ) -> &mut Self;
    fn add_state_tree_global_binding<B, S, M>(
        &mut self,
        name: impl Into<Box<str>>,
        source: S,
    ) -> &mut Self
    where
        B: StateTreeBinding + StateTreeInput,
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

    fn add_state_tree_effect<M: Message + StateTreeInput>(
        &mut self,
        name: impl Into<Box<str>>,
    ) -> &mut Self {
        crate::effect::register_effect::<M>(self, name.into(), RegistrationOrigin::Application);
        self
    }

    fn add_state_tree_activity<A: StateTreeActivity + StateTreeInput>(
        &mut self,
        activity: Activity<A>,
    ) -> &mut Self {
        crate::activity::register_activity(self, activity, RegistrationOrigin::Application);
        self
    }

    fn add_state_tree_global_binding<B, S, M>(
        &mut self,
        name: impl Into<Box<str>>,
        source: S,
    ) -> &mut Self
    where
        B: StateTreeBinding + StateTreeInput,
        S: IntoSystem<(), B::Value, M> + 'static,
    {
        crate::binding::register_global_binding::<B, S, M>(self, name.into(), source);
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

    fn add_state_tree_effect<M: Message + StateTreeInput>(
        &mut self,
        name: impl Into<Box<str>>,
    ) -> &mut Self {
        self.world_mut().add_state_tree_effect::<M>(name);
        self
    }

    fn add_state_tree_activity<A: StateTreeActivity + StateTreeInput>(
        &mut self,
        activity: Activity<A>,
    ) -> &mut Self {
        self.world_mut().add_state_tree_activity(activity);
        self
    }

    fn add_state_tree_global_binding<B, S, M>(
        &mut self,
        name: impl Into<Box<str>>,
        source: S,
    ) -> &mut Self
    where
        B: StateTreeBinding + StateTreeInput,
        S: IntoSystem<(), B::Value, M> + 'static,
    {
        self.world_mut()
            .add_state_tree_global_binding::<B, S, M>(name, source);
        self
    }
}

pub(crate) fn add_message<M: Message>(world: &mut World) {
    if !world.contains_resource::<Messages<M>>() {
        MessageRegistry::register_message::<M>(world);
    }
}
