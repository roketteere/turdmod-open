# cosmetic-icon-pack

> **Warning: Engine Required — Not Functional on Stock SCUM Today**
>
> Cosmetic icon packs are UE4 `.pak` content mods that need to be loaded into
> the running SCUM process. SCUM has no published mod loader for this, so
> custom paks have no install mechanism today.
>
> This will work when the **TurdMOD Engine** ships (DLL injection + UE4
> pak-loading hook). Until then, this is a preview of the pak-content
> authoring format.

**Status:** Preview — needs TurdMOD Engine (target: late Q2 2026)

A TurdMOD mod (`pak-content`).

## Build & install

```bash
# validate the manifest
turdmod pak validate ./turdmod.json

# (todo) cook your assets into a .pak with the UE editor / unrealpak,
# then install it:
turdmod pak install ./dist/cosmetic-icon-pack.pak
```

## Files

- `turdmod.json` — manifest
- `assets/` — UE 4.27 assets that get baked into a .pak
- `README.md` — this file

See [TurdMOD docs](../../docs/turdmod/) for the API surface and policy details.
