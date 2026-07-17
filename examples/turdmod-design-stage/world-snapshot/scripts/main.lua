-- World Snapshot
--
-- Press the hotkey, write every actor in the running SCUM world to a
-- timestamped JSON file. Useful for:
--   * Auditing vehicle / NPC populations live (verifies #07 vehicles doc).
--   * Mapping spawn points the .pak doesn't carry statically.
--   * Comparing world state across builds (drop into data/version-diffs/).
--   * Just plain "what the hell does the engine actually have right now".
--
-- This mod is the first real consumer of the engine.* surface that #169
-- exposes. Today the engine surface stubs out (#169 hasn't wired actor
-- enumeration yet), so this mod logs a "would dump N actors" line and
-- writes a placeholder file. When #169 lands, the same Lua runs against
-- real data with no changes.

turdmod.log("info", "world-snapshot loaded")

local function snapshot()
  local ts = os.date("!%Y%m%dT%H%M%SZ")
  turdmod.log("info", "world snapshot starting @ " .. ts)

  -- engine.actors() will be the new Lua-side surface introduced by #169.
  -- It returns a flat list of tables: { class, name, location, owner,
  -- replicated_props_subset, ... }. Until then we self-report and write a
  -- placeholder so the hotkey wiring + file path are exercised today.
  local actors = (turdmod.engine and turdmod.engine.actors and turdmod.engine.actors()) or nil
  if not actors then
    turdmod.log("warn", "engine.actors() unavailable — #169 not wired yet, writing placeholder")
    turdmod.persistence.set("snapshot/" .. ts, {
      ts = ts,
      placeholder = true,
      note = "engine.actors() not implemented; #169 brings this online",
    })
    return
  end

  -- Real path (post-#169):
  local out = {
    ts = ts,
    build = turdmod.engine.build_id and turdmod.engine.build_id() or nil,
    counts = {},
    actors = actors,
  }
  -- Aggregate counts by class for a quick overview at the top of the file.
  for _, a in ipairs(actors) do
    out.counts[a.class] = (out.counts[a.class] or 0) + 1
  end
  turdmod.persistence.set("snapshot/" .. ts, out)
  turdmod.log("info", "world snapshot wrote " .. tostring(#actors) .. " actors")
end

-- Hotkey registration. The `ui.hotkey` API also waits on #169; for now
-- this is a noop registration and we provide a manual `turdmod.dispatch`
-- entry-point so the snapshot can be triggered via the loader's
-- `turdmod_loader_dispatch` C export (used by `turdmod doctor` + tests).
if turdmod.ui and turdmod.ui.hotkey then
  turdmod.ui.hotkey("F9", snapshot)
  turdmod.log("info", "hotkey F9 registered")
else
  turdmod.log("warn", "ui.hotkey unavailable — falling back to manual dispatch on `world.snapshot.fire`")
end

-- Either way, accept dispatched fires (companion-IPC, cli doctor, etc.).
turdmod.on("world.snapshot.fire", function(_)
  snapshot()
end)
