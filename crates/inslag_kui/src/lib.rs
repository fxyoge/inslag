//! Author Bevy interfaces in KUI, bind them to Luau values, and control them with SCXML.
//!
//! # Author a view
//!
//! Save this as `assets/ui/menu.kui`:
//!
//! ```xml
//! <view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1"
//!       version="1" name="menu">
//!   <!-- Flair handles layout and styling. -->
//!   <style src="ui/menu.css"/>
//!   <node class="menu">
//!     <text>Choose a tree</text>
//!     <!-- bind:* evaluates Luau against the chart's data. -->
//!     <button on-click="choose" bind:value="tree_id">
//!       <text bind:text="tree_id"/>
//!     </button>
//!     <!-- Keep copy and number formatting in the view. -->
//!     <text template="Coins: {coins:grouped}"/>
//!     <!-- Visibility can depend on an expression too. -->
//!     <text template="Selected: {selected_tree}" bind:visible="selected_tree ~= ''"/>
//!   </node>
//! </view>
//! ```
//!
//! Save this as `assets/ui/menu.css`:
//!
//! ```css
//! .menu {
//!     flex-direction: column;
//!     row-gap: 8px;
//!     padding: 16px;
//! }
//! button { padding: 8px; background-color: #345678; }
//! button:hover { background-color: #456789; }
//! ```
//!
//! # Control it with SCXML
//!
//! Save this as `assets/ui/menu.scxml`:
//!
//! ```xml
//! <scxml xmlns="http://www.w3.org/2005/07/scxml"
//!        xmlns:ist="urn:inslag-state-tree" datamodel="luau" initial="active">
//!   <datamodel>
//!     <data id="tree_id" expr="'oak'"/>
//!     <data id="coins" expr="1250"/>
//!     <data id="selected_tree" expr="''"/>
//!   </datamodel>
//!   <state id="active">
//!     <ist:activity id="menu" name="ui.render">
//!       <ist:arg name="src" expr="'ui/menu.kui'"/>
//!     </ist:activity>
//!     <transition event="ui.action" cond="_event.data.action == 'choose'">
//!       <assign location="selected_tree" expr="_event.data.value"/>
//!     </transition>
//!   </state>
//! </scxml>
//! ```
//!
//! The view stays mounted while `ui.render` is active. Leaving the state removes it.
//!
//! # Run it
//!
//! Add [`state_tree::KuiStateTreePlugin`] after the state-tree and Flair plugins:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use bevy_flair::FlairPlugin;
//! use inslag_kui::state_tree::KuiStateTreePlugin;
//! use inslag_state_tree::{StateTreeInstance, StateTreePlugin};
//!
//! App::new()
//!     .add_plugins((
//!         DefaultPlugins,
//!         FlairPlugin,
//!         StateTreePlugin::new(Update),
//!         KuiStateTreePlugin,
//!     ))
//!     .add_systems(Startup, |mut commands: Commands, assets: Res<AssetServer>| {
//!         commands.spawn(Camera2d);
//!         commands.spawn(StateTreeInstance::new(assets.load("ui/menu.scxml")));
//!     })
//!     .run();
//! ```
//!
//! See [`state_tree`] for focus and activation, and [`source`] for parsing and validation.
//! Enable the `aseprite` feature to use Aseprite slices in styles.
#[cfg(doctest)]
mod api_tests;
mod asset;
#[cfg(test)]
mod component_tests;
mod interaction;
mod motion;
mod path;
mod runtime;
#[cfg(test)]
mod tests;
mod text;

#[cfg(feature = "aseprite")]
pub mod aseprite;
pub mod state_tree;

#[cfg(test)]
pub(crate) use asset::parse;
pub(crate) use asset::{
    KuiAsset, KuiAssetLoader, KuiBinding, KuiNode, KuiNodeKind, KuiProp, KuiPropSource,
    validate_asset_path,
};
pub(crate) use interaction::KuiActionData;
pub(crate) use runtime::{
    KuiAction, KuiComponentInstance, KuiPlugin, KuiSystems, KuiValue, KuiView, KuiViewRoot,
    KuiViewStatus,
};
#[cfg(test)]
pub(crate) use runtime::{KuiItemKey, KuiRepeatItem};
pub(crate) use text::{KuiTextFormat, KuiTextPart};
mod expression;

/// Parse KUI for editors and asset checks.
///
/// ```
/// use inslag_kui::source::parse;
///
/// let view = parse("ui/menu.kui", r#"
/// <view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1"
///       version="1" name="menu">
///   <text bind:text="title"/>
/// </view>
/// "#)?;
/// assert_eq!(view.name, "menu");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// Use [`compile`](crate::source::compile) to resolve imported components from a map of source files.
pub mod source {
    pub use crate::asset::compile_sources as compile;
    pub use crate::asset::{
        KuiBinding, KuiComponentContract, KuiDefinition, KuiLoaderError, KuiNode, KuiNodeKind,
        KuiParseError, KuiProp, KuiPropSource, KuiPropType, KuiStyleDefinition, parse,
        validate_asset_path,
    };
    pub use crate::expression::environment as expression_environment;
    pub use crate::path::binding_roots;
    pub use crate::text::{KuiTextFormat, KuiTextPart, parse_template as parse_text_template};
}
