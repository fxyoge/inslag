# inslag_loot

Asset-backed random grab bags. Author weighted choices, reusable tables, and relationships between generated results; consume the completed bag however your game needs.

Use it for loot, crafting, corruption, encounter compositions, and other situations where independent drops aren't enough.

Disclaimer: this crate is not production ready. Expect breaking changes.

## Example

```text
table "forest-encounter" using=actor
  azure_slime role=leader
  choose rolls=2 distinct=true
    #green-escort
    #violet-escort
    #amber-escort
```

```rust
use bevy_app::{App, Startup, TaskPoolPlugin, Update};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use inslag_loot::prelude::*;

#[derive(Clone, Debug, PartialEq, LootArgs, LootOutput)]
struct ActorSpec {
    #[loot(primary)]
    definition: String,
    #[loot(default = "member".into())]
    role: String,
}

fn request(mut rolls: MessageWriter<LootRequest<ActorSpec>>) {
    rolls.write(LootRequest::new("forest-encounter"));
}

fn receive(mut rolls: MessageReader<LootFinished<ActorSpec>>) {
    for roll in rolls.read() {
        println!("{:?}", roll.result);
    }
}

let mut app = App::new();
app.add_plugins((
    TaskPoolPlugin::default(),
    AssetPlugin::default(),
    LootPlugin::default().with_sources(["encounters.loot"]),
))
.add_loot_function("actor", |actor: ActorSpec| actor)
.add_loot_output::<ActorSpec>()
.add_systems(Startup, request)
.add_systems(Update, receive);
```

See [examples/](examples/) for more.
