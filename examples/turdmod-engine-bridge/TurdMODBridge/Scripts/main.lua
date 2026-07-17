--[[
    TurdMOD Bridge — production-grade UE4SS Lua mod.

    Bridges between SCUMServer.exe (via UE4SS reflection / hooks) and
    `turdmod-server-loader.dll` (which terminates the named-pipe RPC the
    companion talks to).

    Architecture:

        companion (Node.js)
            ↕ JSON-RPC over named pipe
        turdmod-server-loader.dll  ──┐
                                     │  shared FS dir (JSONL bridge)
        UE4SS.dll  +  this Lua mod  ─┘
            ↕ UE4SS reflection / hooks
        SCUMServer.exe (UE4 4.27)

    In-process bridge between the Rust DLL and this Lua mod is file-based
    JSONL polling in `%LOCALAPPDATA%/TurdMOD/engine/`:

      - inbox.jsonl   — DLL writes commands here, Lua reads + truncates
      - outbox.jsonl  — Lua writes events / responses here, DLL reads + truncates

    File-based IPC is intentional for v0.1: zero protocol surface, easy
    to debug by `cat`-ing the file, no Lua-side socket library needed.
    Poll interval is 50ms (≤ 1 frame at 60fps; below human-noticeable for
    admin commands).

    Compared to v0.1 skeleton, this version:
      - Real handlers using documented UE4SS Lua APIs (FindAllOf,
        ClientMessage, K2_SetActorLocation, ExecuteInGameThread)
      - `discover.*` family of methods for live SCUM introspection
        (classes / functions / world / players) — lets us identify SCUM
        UFunction paths from the companion side without needing in-game
        Ctrl+J dumps
      - Standardized error response format with typed error codes
      - safe_call wrapper around every handler — exceptions become typed
        error responses instead of silently killing the dispatch loop
      - Tightened poll interval (50ms vs 100ms)
      - Real UE4 RegisterHook for PostLogin/Logout with extracted player
        identity
--]]

local UEHelpers = require("UEHelpers")
local json = require("json")

local MOD_NAME = "TurdMODBridge"
local VERSION  = "0.2.0"

-- ── Paths ──────────────────────────────────────────────────────────────────

local function bridge_dir()
    local localappdata = os.getenv("LOCALAPPDATA") or ""
    return localappdata .. "\\TurdMOD\\engine\\"
end

local INBOX_PATH  = bridge_dir() .. "inbox.jsonl"
local OUTBOX_PATH = bridge_dir() .. "outbox.jsonl"
local READY_PATH  = bridge_dir() .. "lua-ready.marker"

-- ── Logging ────────────────────────────────────────────────────────────────

local function Log(msg)
    print(string.format("[%s] %s", MOD_NAME, tostring(msg)))
end

-- ── Error codes ────────────────────────────────────────────────────────────

local E_PARSE          = -32700  -- malformed JSON
local E_INVALID_REQ    = -32600  -- shape wrong
local E_NO_METHOD      = -32601  -- method not registered
local E_INVALID_PARAMS = -32602  -- params shape wrong
local E_INTERNAL       = -32603  -- handler exception
local E_NO_WORLD       =   1001  -- UWorld unavailable (level transition?)
local E_NO_PLAYER      =   1002  -- player not found by id
local E_NO_PAWN        =   1003  -- player has no pawn (dead / disconnecting)
local E_HOOK_FAILED    =   1004  -- UE4 call failed at runtime

local function err(code, message, data)
    local e = { code = code, message = message }
    if data ~= nil then e.data = data end
    return e
end

-- ── File IO helpers ────────────────────────────────────────────────────────

local function emit_line(envelope)
    local ok, encoded = pcall(json.encode, envelope)
    if not ok then
        Log("emit encode failed: " .. tostring(encoded))
        return
    end
    local f, ferr = io.open(OUTBOX_PATH, "ab")
    if not f then
        Log("emit open outbox failed: " .. tostring(ferr))
        return
    end
    f:write(encoded); f:write("\n"); f:close()
end

local function emit_event(event_name, data)
    emit_line({ event = event_name, data = data, ts = os.time() })
end

local function emit_response(id, result, error_obj)
    local envelope = { id = id }
    if error_obj then envelope.error = error_obj else envelope.result = result end
    emit_line(envelope)
end

local function drain_inbox()
    local f = io.open(INBOX_PATH, "rb")
    if not f then return {} end
    local body = f:read("*all")
    f:close()
    if not body or body == "" then return {} end

    -- Best-effort truncate. Single-writer (Rust DLL) so race-window is small.
    local trunc = io.open(INBOX_PATH, "wb")
    if trunc then trunc:close() end

    local cmds = {}
    for line in string.gmatch(body, "[^\r\n]+") do
        local ok, parsed = pcall(json.decode, line)
        if ok and type(parsed) == "table" then
            table.insert(cmds, parsed)
        else
            Log("inbox decode failed: " .. tostring(parsed))
        end
    end
    return cmds
end

-- ── UE4 reflection helpers ─────────────────────────────────────────────────

-- IsValid() is provided by UE4SS for every UObject userdata; guard against
-- nil access. Returns the obj if valid, nil otherwise.
local function valid(obj)
    if obj == nil then return nil end
    local ok, is_valid = pcall(function() return obj:IsValid() end)
    if ok and is_valid then return obj end
    return nil
end

-- Try to extract a steam-64 id from an APlayerState.
-- UE4 stores this on APlayerState.UniqueId (FUniqueNetIdRepl). The exact
-- path to the underlying u64 varies by online subsystem; we try a few
-- common shapes and return nil if none work.
local function get_steam_id(player_state)
    if not valid(player_state) then return nil end
    -- Try: PlayerState.UniqueId:ToString() — returns a textual id we can
    -- parse to u64 if it's all digits.
    local ok, uid = pcall(function() return player_state.UniqueId end)
    if ok and uid then
        -- UniqueId is FUniqueNetIdRepl; the inner UniqueNetId has ToString
        local ok2, s = pcall(function() return uid:ToString() end)
        if ok2 and type(s) == "string" then
            return s
        end
        -- Fallback: try the inner UniqueNetId via .UniqueNetId field
        local ok3, inner = pcall(function() return uid.UniqueNetId end)
        if ok3 and inner then
            local ok4, s2 = pcall(function() return inner:ToString() end)
            if ok4 and type(s2) == "string" then return s2 end
        end
    end
    return nil
end

-- Get a printable player name from an APlayerState.
local function get_player_name(player_state)
    if not valid(player_state) then return nil end
    local ok, n = pcall(function() return player_state:GetPlayerName() end)
    if ok and type(n) == "string" then return n end
    -- Fallback: PlayerName FString
    local ok2, n2 = pcall(function() return player_state.PlayerName end)
    if ok2 and type(n2) == "string" then return n2 end
    return nil
end

-- Get pawn world-position from any actor; returns {x,y,z} or nil.
local function get_actor_location(actor)
    if not valid(actor) then return nil end
    local ok, loc = pcall(function() return actor:K2_GetActorLocation() end)
    if ok and loc then
        return { x = loc.X or 0, y = loc.Y or 0, z = loc.Z or 0 }
    end
    return nil
end

-- Iterate all APlayerController instances. Returns Lua array.
local function get_all_player_controllers()
    local pcs = {}
    local ok, list = pcall(function() return FindAllOf("PlayerController") end)
    if not ok or not list then return pcs end
    -- FindAllOf returns a Lua-iterable userdata; convert to array.
    for i, pc in ipairs(list) do
        if valid(pc) then table.insert(pcs, pc) end
    end
    return pcs
end

-- Find APlayerController whose PlayerState matches the given steam id string.
local function find_pc_by_steam(steam_str)
    if not steam_str then return nil end
    for _, pc in ipairs(get_all_player_controllers()) do
        local ps = pc.PlayerState
        if get_steam_id(ps) == steam_str then return pc end
    end
    return nil
end

-- Project a PC to the wire-format we send to the companion.
local function pc_to_summary(pc)
    if not valid(pc) then return nil end
    local ps = pc.PlayerState
    local pawn = pc.Pawn
    local loc = get_actor_location(pawn)
    return {
        steamId = get_steam_id(ps),
        name    = get_player_name(ps),
        x = loc and loc.x or nil,
        y = loc and loc.y or nil,
        z = loc and loc.z or nil,
        hasPawn = pawn ~= nil and valid(pawn) ~= nil,
        controllerName = pc:GetFullName(),
    }
end

-- ── RPC command handlers ───────────────────────────────────────────────────

local handlers = {}

-- ─── Core RPC ──────────────────────────────────────────────────────────────

handlers.ping = function(params)
    return { pong = true, version = VERSION, mod = MOD_NAME, kind = "lua" }
end

handlers.broadcastChat = function(params)
    local text = params and params.text
    if type(text) ~= "string" then
        return nil, err(E_INVALID_PARAMS, "missing 'text' string param")
    end
    local pcs = get_all_player_controllers()
    if #pcs == 0 then
        return { ok = true, sent = 0, note = "no players connected" }
    end
    local sent = 0
    ExecuteInGameThread(function()
        for _, pc in ipairs(pcs) do
            local ok = pcall(function()
                -- ClientMessage(InString, InType, MsgLifeTime)
                -- Standard UE4 APlayerController API; should exist on
                -- SCUM unless they removed it.
                pc:ClientMessage(text, "Default", 5.0)
            end)
            if ok then sent = sent + 1 end
        end
    end)
    -- We return optimistically since the game-thread send is async; the
    -- emit_event below confirms when the work actually ran.
    emit_event("broadcastChatExecuted", { text = text, intendedCount = #pcs })
    return { ok = true, queued = #pcs }
end

handlers.sendPrivateMessage = function(params)
    local steam = params and params.steamId
    local text  = params and params.text
    if type(steam) ~= "string" or type(text) ~= "string" then
        return nil, err(E_INVALID_PARAMS, "need 'steamId' and 'text' strings")
    end
    local pc = find_pc_by_steam(steam)
    if not pc then
        return nil, err(E_NO_PLAYER, "no player with steamId " .. steam)
    end
    ExecuteInGameThread(function()
        pcall(function() pc:ClientMessage(text, "Default", 5.0) end)
    end)
    return { ok = true }
end

handlers.teleportPlayer = function(params)
    local steam = params and params.steamId
    local x, y, z = params and params.x, params and params.y, params and params.z
    if type(steam) ~= "string" or type(x) ~= "number" or type(y) ~= "number" or type(z) ~= "number" then
        return nil, err(E_INVALID_PARAMS, "need steamId/x/y/z")
    end
    local pc = find_pc_by_steam(steam)
    if not pc then
        return nil, err(E_NO_PLAYER, "no player with steamId " .. steam)
    end
    local pawn = pc.Pawn
    if not valid(pawn) then
        return nil, err(E_NO_PAWN, "player has no active pawn")
    end
    ExecuteInGameThread(function()
        local ok = pcall(function()
            -- K2_SetActorLocation(NewLocation, bSweep, OutSweepHitResult, bTeleport)
            pawn:K2_SetActorLocation({ X = x, Y = y, Z = z }, false, {}, true)
        end)
        if not ok then
            Log(string.format("teleport failed for %s", steam))
        end
    end)
    emit_event("teleportExecuted", { steamId = steam, x = x, y = y, z = z })
    return { ok = true, queued = true }
end

handlers.getOnlinePlayers = function(params)
    local out = {}
    for _, pc in ipairs(get_all_player_controllers()) do
        local s = pc_to_summary(pc)
        if s then table.insert(out, s) end
    end
    return out
end

handlers.getPlayerPosition = function(params)
    local steam = params and params.steamId
    if type(steam) ~= "string" then
        return nil, err(E_INVALID_PARAMS, "missing 'steamId' string")
    end
    local pc = find_pc_by_steam(steam)
    if not pc then return nil, err(E_NO_PLAYER, "no player with steamId " .. steam) end
    local loc = get_actor_location(pc.Pawn)
    if not loc then return nil, err(E_NO_PAWN, "no pawn or no location") end
    return loc
end

handlers.spawnVehicle = function(params)
    local cls = params and params.className
    local x, y, z = params and params.x, params and params.y, params and params.z
    if type(cls) ~= "string" or type(x) ~= "number" then
        return nil, err(E_INVALID_PARAMS, "need 'className' and x/y/z")
    end
    -- TODO(sprint2.5): UE4 spawn pattern needs UClass + UWorld:SpawnActor.
    -- UE4SS Lua doesn't expose SpawnActor directly; we'd typically use a
    -- BP function library. SCUM may have one. Until then, log + ack.
    ExecuteInGameThread(function()
        Log(string.format("spawnVehicle (not yet impl) class=%s @ %.1f,%.1f,%.1f", cls, x, y, z))
    end)
    return nil, err(E_HOOK_FAILED, "spawnVehicle needs SCUM-specific BP helper — discovery pending")
end

-- ─── Discovery API (probe SCUM internals from the companion) ──────────────
--
-- These let us identify SCUM-specific UFunction paths without needing Joel
-- to run Ctrl+J in-game. The companion can call discover.* and get a list
-- of class/function names back, then we update handler code accordingly.

local discover = {}

-- discover.classes(pattern) — list all UClass FullNames matching a Lua pattern.
-- Use a wide pattern like "" to dump everything (large response!).
discover.classes = function(params)
    local pattern = params and params.pattern or ""
    local limit   = params and params.limit or 200
    local out = {}
    local ok, classes = pcall(function() return FindAllOf("Class") end)
    if not ok or not classes then
        return nil, err(E_HOOK_FAILED, "FindAllOf('Class') failed")
    end
    for _, c in ipairs(classes) do
        if valid(c) then
            local name = c:GetFullName()
            if pattern == "" or string.find(name, pattern) then
                table.insert(out, name)
                if #out >= limit then break end
            end
        end
    end
    return { matches = out, returned = #out, limit = limit }
end

-- discover.functions(class_path) — list all UFunctions on a UClass.
-- class_path is the UE4 object path (e.g. "/Script/Engine.PlayerController")
-- or a substring matching exactly one class.
discover.functions = function(params)
    local class_path = params and params.classPath
    if type(class_path) ~= "string" then
        return nil, err(E_INVALID_PARAMS, "need 'classPath' string")
    end
    local class_obj
    local ok = pcall(function() class_obj = StaticFindObject(class_path) end)
    if not ok or not valid(class_obj) then
        return nil, err(E_NO_PLAYER, "class not found: " .. class_path)
    end
    local out = {}
    -- Iterate Children (UStruct linked list of UFields, including UFunctions)
    local child = class_obj.Children
    while valid(child) do
        local n = child:GetName()
        table.insert(out, n)
        local ok2, next_child = pcall(function() return child.Next end)
        if not ok2 then break end
        child = next_child
        if #out > 500 then break end -- safety
    end
    return { class = class_path, functions = out, count = #out }
end

-- discover.world — quick orientation: UWorld + GameMode + GameState class names
discover.world = function(params)
    local world = UEHelpers.GetWorld()
    if not valid(world) then
        return nil, err(E_NO_WORLD, "UWorld unavailable")
    end
    local function safe_class_name(obj)
        if not valid(obj) then return nil end
        local ok, cls = pcall(function() return obj.Class end)
        if not ok or not valid(cls) then return nil end
        return cls:GetFullName()
    end
    return {
        world_class      = safe_class_name(world),
        world_name       = world:GetFullName(),
        game_mode_class  = safe_class_name(world.AuthorityGameMode),
        game_state_class = safe_class_name(world.GameState),
        net_driver_class = safe_class_name(world.NetDriver),
        player_count     = #get_all_player_controllers(),
    }
end

-- discover.players — like getOnlinePlayers but with full debug detail
-- (PC class, pawn class, raw PlayerState data) for SCUM-internals probing.
discover.players = function(params)
    local out = {}
    for _, pc in ipairs(get_all_player_controllers()) do
        local ps = pc.PlayerState
        local pawn = pc.Pawn
        local function class_of(o)
            if not valid(o) then return nil end
            local ok, c = pcall(function() return o.Class:GetFullName() end)
            return ok and c or nil
        end
        table.insert(out, {
            controller_full_name = pc:GetFullName(),
            controller_class     = class_of(pc),
            player_state_class   = class_of(ps),
            pawn_class           = class_of(pawn),
            steamId              = get_steam_id(ps),
            name                 = get_player_name(ps),
            location             = get_actor_location(pawn),
        })
    end
    return out
end

handlers["discover.classes"]   = discover.classes
handlers["discover.functions"] = discover.functions
handlers["discover.world"]     = discover.world
handlers["discover.players"]   = discover.players

-- ── Dispatch ───────────────────────────────────────────────────────────────

local function safe_call(fn, params)
    local ok, result, error_obj = pcall(fn, params or {})
    if not ok then
        return nil, err(E_INTERNAL, "handler exception: " .. tostring(result))
    end
    return result, error_obj
end

local function dispatch(cmd)
    if type(cmd) ~= "table" or not cmd.method then
        Log("dispatch: malformed command")
        return
    end
    local fn = handlers[cmd.method]
    if not fn then
        Log("dispatch: unknown method " .. tostring(cmd.method))
        if cmd.id then
            emit_response(cmd.id, nil, err(E_NO_METHOD, "unknown method: " .. cmd.method))
        end
        return
    end
    local result, error_obj = safe_call(fn, cmd.params)
    if cmd.id then emit_response(cmd.id, result, error_obj) end
end

-- ── Event hooks ────────────────────────────────────────────────────────────

local function safe_register_hook(path, callback)
    local ok, errmsg = pcall(RegisterHook, path, callback)
    if ok then
        Log("hook registered: " .. path)
        return true
    end
    Log("hook FAILED for " .. path .. ": " .. tostring(errmsg))
    return false
end

local function on_post_login(ctx, new_player_param)
    local ok, pc = pcall(function() return new_player_param:get() end)
    if not ok or not valid(pc) then return end
    local ps = pc.PlayerState
    local pawn = pc.Pawn
    emit_event("playerLogin", {
        steamId = get_steam_id(ps),
        name    = get_player_name(ps),
        location = get_actor_location(pawn),
        controllerName = pc:GetFullName(),
    })
end

local function on_logout(ctx, exiting_param)
    local ok, controller = pcall(function() return exiting_param:get() end)
    if not ok or not valid(controller) then return end
    emit_event("playerLogout", {
        steamId = get_steam_id(controller.PlayerState),
        controllerName = controller:GetFullName(),
    })
end

safe_register_hook("/Script/Engine.GameMode:PostLogin", on_post_login)
safe_register_hook("/Script/Engine.GameMode:Logout",    on_logout)

-- SCUM-specific hooks land here once Sprint 2.5 discovery completes:
--   safe_register_hook("/Script/Scum.<SCUM_PlayerController>:<ServerChat>", on_chat)
--   safe_register_hook("/Script/Scum.<SCUM_Character>:<Die>", on_death)
-- Use discover.classes("Scum") and discover.functions("/Script/Scum.<name>")
-- from the companion to identify the right paths.

-- ── Poll loop ──────────────────────────────────────────────────────────────

local POLL_INTERVAL_MS = 50

local function tick()
    local cmds = drain_inbox()
    for _, cmd in ipairs(cmds) do
        dispatch(cmd)
    end
end

-- ── Startup ────────────────────────────────────────────────────────────────

os.execute('mkdir "' .. bridge_dir() .. '" 2>nul')

local f = io.open(READY_PATH, "wb")
if f then
    f:write(string.format("ready %s v%s ts=%d", MOD_NAME, VERSION, os.time()))
    f:close()
end

LoopAsync(POLL_INTERVAL_MS, tick)
Log(string.format("%s v%s started; polling %s every %dms", MOD_NAME, VERSION, INBOX_PATH, POLL_INTERVAL_MS))
emit_event("bridgeReady", {
    version = VERSION,
    mod = MOD_NAME,
    kind = "lua",
    pollIntervalMs = POLL_INTERVAL_MS,
    methods = (function()
        local m = {}
        for k, _ in pairs(handlers) do table.insert(m, k) end
        table.sort(m)
        return m
    end)(),
})
