-- The public API is authored in Luau; Rust implements only generic operations.
-- Never put a scenario, fixture, or game-specific assertion in the Rust adapter.
return function(running, timeout)
    local function rpc(operation, arguments)
        return coroutine.yield({ operation = operation, arguments = arguments or {} })
    end

    local context = {}
    function context.frames(count) rpc("step", { count = count or 1 }) end
    function context.query(spec) return rpc("query", spec or {}) end
    function context.component(entity, typeName)
        return rpc("component", { entity = entity, type = typeName })
    end
    function context.resource(typeName) return rpc("resource", { type = typeName }) end
    function context.tree(entity, key)
        return rpc("tree", { entity = entity, key = key })
    end
    function context.screen(entity) return rpc("screen", { entity = entity }) end
    function context.ready() return rpc("ready") end
    function context.exiting() return rpc("exiting") end
    function context.saveStatus() return rpc("save_status") end
    function context.waitUntil(predicate, description)
        for attempt = 0, timeout do
            local value = predicate()
            if value then return value end
            if attempt < timeout then context.frames(1) end
        end
        error(`timed out after {timeout} frames: {description or "condition"}`, 2)
    end
    function context.awaitReady()
        context.waitUntil(context.ready, "authored assets and views ready")
        context.frames(3) -- explicit layout/transform settling, never hidden in run()
    end

    if not running then
        function context.spawn(name) return rpc("spawn", { name = name }) end
        function context.despawn(entity) rpc("despawn", { entity = entity }) end
        function context.parent(entity, parent) rpc("parent", { entity = entity, parent = parent }) end
        function context.insert(entity, typeName, value)
            rpc("insert", { entity = entity, type = typeName, value = value })
        end
        function context.remove(entity, typeName) rpc("remove", { entity = entity, type = typeName }) end
        function context.setResource(typeName, value) rpc("set_resource", { type = typeName, value = value }) end
        function context.setAsset(path, value)
            context.waitUntil(function()
                return rpc("set_asset", { path = path, value = value })
            end, `asset {path} loaded for replacement`)
        end
        function context.copyAsset(path, source)
            context.waitUntil(function()
                return rpc("copy_asset", { path = path, source = source })
            end, `fixture {source} copied to {path}`)
        end
        local function merge(value, fields)
            for key, child in fields do
                if type(child) == "table" and type(value[key]) == "table" then
                    merge(value[key], child)
                else
                    value[key] = child
                end
            end
            return value
        end
        function context.patch(entity, typeName, fields)
            context.insert(entity, typeName, merge(context.component(entity, typeName), fields))
        end
        function context.patchResource(typeName, fields)
            context.setResource(typeName, merge(context.resource(typeName), fields))
        end
        function context.createTree(entity, asset, data, config)
            rpc("create_tree", { entity = entity, asset = asset, data = data, config = config })
        end
    end

    local input = {}
    function input.keyDown(key, text) rpc("input", { kind = "key", key = key, down = true, text = text }) end
    function input.keyUp(key) rpc("input", { kind = "key", key = key, down = false }) end
    function input.tapKey(key)
        input.keyDown(key)
        context.frames(1)
        input.keyUp(key)
        context.frames(1)
    end
    function input.cursor(x, y) rpc("input", { kind = "cursor", x = x, y = y }) end
    function input.mouseDown(button) rpc("input", { kind = "button", button = button or "Left", down = true }) end
    function input.mouseUp(button) rpc("input", { kind = "button", button = button or "Left", down = false }) end
    function input.wheel(x, y) rpc("input", { kind = "wheel", x = x, y = y }) end
    function input.focus(focused) rpc("input", { kind = "focus", focused = focused }) end
    function input.gamepadConnect(gamepad, name)
        rpc("input", { kind = "gamepad_connect", gamepad = gamepad, name = name })
    end
    function input.gamepadDisconnect(gamepad)
        rpc("input", { kind = "gamepad_disconnect", gamepad = gamepad })
    end
    function input.gamepadButton(gamepad, button, value)
        rpc("input", { kind = "gamepad_button", gamepad = gamepad, button = button, value = value })
    end
    function input.gamepadAxis(gamepad, axis, value)
        rpc("input", { kind = "gamepad_axis", gamepad = gamepad, axis = axis, value = value })
    end
    function input.clickAt(point)
        input.cursor(point.x, point.y)
        context.frames(1)
        input.mouseDown()
        context.frames(1)
        input.mouseUp()
        context.frames(2)
    end
    function input.drag(from, to, frames)
        frames = frames or 12
        input.cursor(from.x, from.y)
        context.frames(1)
        input.mouseDown()
        context.frames(1)
        for index = 1, frames do
            local t = index / frames
            input.cursor(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t)
            context.frames(1)
        end
        input.mouseUp()
        context.frames(2)
    end
    context.input = table.freeze(input)

    if not running then return table.freeze(context) end

    local function format(value)
        if type(value) ~= "table" then return tostring(value) end
        local parts = {}
        for key, child in value do table.insert(parts, `{key}={format(child)}`) end
        table.sort(parts)
        return "{" .. table.concat(parts, ", ") .. "}"
    end
    local function equal(left, right)
        if type(left) ~= type(right) then return false end
        if type(left) ~= "table" then return left == right end
        for key, value in left do if not equal(value, right[key]) then return false end end
        for key in right do if left[key] == nil then return false end end
        return true
    end
    function context.expect(actual)
        return {
            toEqual = function(expected)
                if not equal(actual, expected) then
                    error(`expected {format(expected)}, received {format(actual)}`, 2)
                end
                rpc("assertion")
            end,
        }
    end
    function context.expectEventually(observe, expected, description)
        local actual
        for attempt = 0, timeout do
            actual = observe()
            if equal(actual, expected) then
                rpc("assertion")
                return actual
            end
            if attempt < timeout then context.frames(1) end
        end
        error(`{description or "eventual assertion"}: expected {format(expected)}, last received {format(actual)} after {timeout} frames`, 2)
    end
    function context.step(name, callback)
        rpc("label", { name = name })
        callback()
    end
    return table.freeze(context)
end
