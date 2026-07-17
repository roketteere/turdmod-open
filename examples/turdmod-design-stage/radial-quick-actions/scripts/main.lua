-- radial-quick-actions — offline-only TurdMOD script.
--
-- Adds a quick-action radial menu bound to a hotkey: heal a wound,
-- eat a ration, drink water, repair clothes. Demonstrates the
-- ui + player API surfaces in @scummy/turdmod-api.
--
-- Strategy C: runs only in solo or BE-off private servers. The
-- detection layer in apps/turdmod-cli/src/detect.ts gates injection.

local turdmod = require("turdmod")

local HOTKEY = "F4"
local HANDLES = {}

local actions = {
  { id = "heal",   label = "Heal Wound",   item = "BP_Bandage_Item_C",        verb = "use" },
  { id = "eat",    label = "Eat Ration",   item = "BP_MRE_Cheeseburger_C",     verb = "use" },
  { id = "drink",  label = "Drink Water",  item = "BP_BottleOfWater_Item_C",   verb = "use" },
  { id = "repair", label = "Repair Cloth", item = "BP_Sewing_Kit_C",           verb = "use" },
}

local function trigger(action, player)
  turdmod.log("info", "[radial] " .. action.id .. " on " .. player.name)
  -- A real impl would: find the item in the player's inventory and trigger
  -- its `use`. The current API stub only supports give/remove; this is a
  -- demo of the call shape. Real action invocation lands with #169 (UE4 hooks).
  turdmod.ui.toast(action.label .. " (demo — needs real action API)", 2500)
end

local function open_radial()
  local me = turdmod.player.list()[1]
  if not me then return end

  local items = {}
  for _, a in ipairs(actions) do
    table.insert(items, {
      id = a.id,
      label = a.label,
      action = function() trigger(a, me) end,
    })
  end

  table.insert(HANDLES, turdmod.ui.showMenu({
    id = "radial-quick-actions",
    title = "Quick Actions",
    items = items,
  }))
end

local function on_load()
  table.insert(HANDLES, turdmod.ui.bindHotkey(HOTKEY, open_radial))
  turdmod.ui.toast("Radial Quick Actions ready (" .. HOTKEY .. ")", 3000)
end

local function on_unload()
  for _, h in ipairs(HANDLES) do h:dispose() end
  HANDLES = {}
end

return { on_load = on_load, on_unload = on_unload }
