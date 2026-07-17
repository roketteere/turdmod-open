# turdmod-helloworld-pak

**Status (2026-05-22):** placeholder. Phase C of
`docs/plans/priorities-1-7.md`. Will be a UE4 4.27.2 project that
cooks one BP class (`BP_HelloWorld`) into a pak that the bridge can
mount + call.

**Don't create the project files yet.** Phase C kicks off with an
Editor session against borrowed SCUM assets. The full recipe lives at
`docs/runbooks/uproject-cook.md`.

**UE 4.27.2 install — ✅ ALREADY DONE.** `C:/Program Files/Epic Games/UE_4.27/`
(16.8 GB; UnrealPak.exe verified working). No download / install
needed.

**When Phase C starts:**
1. Read `docs/runbooks/uproject-cook.md` top-to-bottom.
2. (UE install ✅ skip — already done.)
3. Use this folder as the UE4 project root.
4. Author `BP_HelloWorld` per the recipe.
5. Cook + pak + deploy.
6. Verify via `node tools/engine-rpc-test.mjs listClassInstances --pattern HelloWorld`.

**Pre-Phase-C constraint:** the pak-bypass (TURDMOD_PAK_BYPASS=1) is
gated on Phase B v3 (caller-aware filtering) to avoid the
`SCUM.uproject` modal. Without that, paks still trigger the modal
even though they load. Phase C is technically viable with v2 + the
modal-tolerated workflow, but a caller-aware v3 is the cleaner path
before shipping any production pak.

**Related:**
- `docs/server-side-custom-ui-plan.md` — P1/P2/P3 sequence this gates
- `docs/pak-mod-investigation-plan.md` — Q2 outcome + recipe origin
- `scripts/pak-probe/build-probe-pak.ps1` — minimal-pak proof
- Memory `pak-bypass-blocks-reflection` — env-var contract
