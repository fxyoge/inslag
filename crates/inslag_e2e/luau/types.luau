--!strict
-- Public harness contracts for authoring/editor assistance. No scenario logic.
export type Entity = string
export type Point = { x: number, y: number }
export type Query = { with: {string}?, without: {string}?, name: string?, owner: Entity? }
export type Row = { id: Entity, name: string? }
export type Observation = {
    frames: (number?) -> (),
    query: (Query?) -> {Row},
    component: (Entity, string) -> any,
    resource: (string) -> any,
    tree: (Entity, string, string?) -> any,
    screen: (Entity) -> Point,
    ready: () -> boolean,
    exiting: () -> boolean,
    saveStatus: () -> { loading: boolean, writable: boolean, load_error: string?, save_error: string?, generation: number? },
    waitUntil: ((() -> any), string?) -> any,
    awaitReady: () -> (),
}
export type Input = {
    keyDown: (string, string?) -> (),
    keyUp: (string) -> (),
    tapKey: (string) -> (),
    cursor: (number, number) -> (),
    mouseDown: (string?) -> (),
    mouseUp: (string?) -> (),
    wheel: (number, number) -> (),
    focus: (boolean) -> (),
    gamepadConnect: (string, string?) -> (),
    gamepadDisconnect: (string) -> (),
    gamepadButton: (string, string, number) -> (),
    gamepadAxis: (string, string, number) -> (),
    clickAt: (Point) -> (),
    drag: (Point, Point, number?) -> (),
}
export type Setup = Observation & {
    spawn: (string?) -> Entity,
    despawn: (Entity) -> (),
    parent: (Entity, Entity) -> (),
    insert: (Entity, string, any) -> (),
    remove: (Entity, string) -> (),
    patch: (Entity, string, any) -> (),
    setResource: (string, any) -> (),
    setAsset: (string, any) -> (),
    copyAsset: (string, string) -> (),
    patchResource: (string, any) -> (),
    createTree: (Entity, string, string, any?, any?) -> (),
    input: Input,
}
export type Run = Observation & {
    input: Input,
    expect: (any) -> { toEqual: (any) -> () },
    expectEventually: ((() -> any), any, string?) -> any,
    step: (string, () -> ()) -> (),
}
export type Scenario<T> = {
    setup: (Setup) -> T,
    run: (Run, T) -> (),
}
return {}
