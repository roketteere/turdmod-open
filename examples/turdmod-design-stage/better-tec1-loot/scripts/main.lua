-- better-tec1-loot — server-side TurdMOD script.
--
-- Quadruples the MaxOccurrences cap on the Gold Memory Module
-- (the rarest tier in TEC1 abandoned-bunker Depository chests).
-- See scummap KTask #156 for the empty-chest investigation that
-- motivated this example.
--
-- Runs in the dedicated server's TurdMOD runtime, not on clients.
-- Vanilla clients connect and see the modded behavior naturally.
--
-- See @scummy/turdmod-api for the full API surface.

local turdmod = require("turdmod")

local OVERRIDE_HANDLES = {}

local function on_load()
  turdmod.log("info", "[better-tec1-loot] applying loot overrides")

  -- Bump cap on the four Memory Module tiers — Gold (Level 4) was
  -- effectively unreachable on default settings.
  for _, tier in ipairs({ 1, 2, 3, 4 }) do
    table.insert(OVERRIDE_HANDLES, turdmod.content.setDataTableOverride({
      ref  = { table = "ItemSpawningParameters", row = "MemoryModule_Level" .. tier },
      values = { MaxOccurrences = -1, CooldownPerSquadMember = { Min = 0.0, Max = 0.0 } },
    }))
  end

  turdmod.log("info", "[better-tec1-loot] loaded; " .. #OVERRIDE_HANDLES .. " overrides active")
end

local function on_unload()
  for _, h in ipairs(OVERRIDE_HANDLES) do h:dispose() end
  OVERRIDE_HANDLES = {}
end

return { on_load = on_load, on_unload = on_unload }
