# SCUM `prisoner.body_simulation` — Metabolism / Base Attributes (CRACKED)

Status: **decoded + write-verified live on OVH 2026-06-07** (TechyRican + Zilla set to 8/5/5/5).
Tool: `tools/scum-attrs.py`. @dep: `SCUM.db` (`prisoner` table). @related: `SCUM-ADMIN-COMMANDS.md`.

## Why this exists — `#SetAttributes` is gated above owner level
SCUM admin commands carry a `_requiredExecutorLevel`. The enum (from engine reflection):

```
EExecutorStatus: Regular(0) < Admin(1) < SuperAdmin(2) < Elevated(3) < Developer(4)
```

- Owner (`ServerSettingsAdminUsers.ini`) = **SuperAdmin(2)**. Listed admins (`AdminUsers.ini`) = **Admin(1)**.
- Normal commands (`#SetGodMode`, `#Teleport`, `#Spawn*`) need Admin/SuperAdmin → work.
- **`#SetAttributes` needs Elevated(3)/Developer(4)** → returns "not authorized" *even for the owner*.
  It's a developer/single-player command; there is no admin-file grant for it on a live MP server.

So to set base attributes on a dedicated server, write the save data directly. That's this format.

## Where attributes live
`prisoner.body_simulation` is a UE-serialized blob, class `/Script/SCUM.PrisonerBodySimulationSave`
(the in-game **Metabolism** struct). Header: `++scum+release-<ver>` + class name, then a tagged
UE property list. The four base attributes are the first properties:

| Property | Type | What |
|---|---|---|
| `BaseStrength` | DoubleProperty | STR (max 8 in-game) |
| `BaseConstitution` | DoubleProperty | CON (max 5) |
| `BaseDexterity` | DoubleProperty | DEX (max 5) |
| `BaseIntelligence` | DoubleProperty | INT (max 5) |

Then: `InitialAge`(Float), `LifeTimeSinceInitialization`/`LifeTimeSinceSpawn`(Double),
`TimeOfDeath`/`TimeOfRevive`(Int64), `Stamina`/`HeartRate`/`BreathingRate`/`OxygenSaturation`/
`BodyTemperature`(Float), then `BodyEffects` (ArrayProperty of structs — blob size varies here, which
is why total blob length differs per prisoner, but the attribute offsets up front stay fixed).

## Tagged-property layout (per property)
```
[i32 name_len][name bytes + \0][i32 type_len]["DoubleProperty" + \0][i64 value_size=8][u8 guid_flag=0][f64 value]
```
Value is **float64 little-endian** (DoubleProperty — NOT float32). Stable value offsets for a fresh
header: Str=128, Con=185, Dex=239, Int=296. `scum-attrs.py` re-derives them by name regardless.

## Editing procedure (server MUST be stopped)
SCUM holds the prisoner in memory and rewrites the blob on save/logout — a live edit gets clobbered.

```
1. stop server          (service API :9090 POST /server/stop)
2. back up SCUM.db (+ -wal + -shm)
3. pull SCUM.db + SCUM.db-wal + SCUM.db-shm TOGETHER  (sqlite applies the WAL on open)
4. tools/scum-attrs.py set SCUM.db <pid> 8 5 5 5      (folds WAL via wal_checkpoint(TRUNCATE))
5. push SCUM.db back; DELETE the server's stale -wal + -shm (else pre-edit frames re-apply)
6. start server         (POST /server/start)
7. player relogs -> Metabolism screen shows new values (attributes load at character spawn)
```

@inv: prisoner_id, NOT user_profile_id, keys the `prisoner` table. Map via
`user_profile.prisoner_id` -> name/steam (`scum-attrs.py whois`).
@brk: if the WAL is not pulled with the db (or not deleted on push), you read/write stale data.
@note: with `BodySimulationSpeedMultiplier > 1`, base attributes can drift over time as the sim
runs; pin via an engine guard if they must stay maxed.
