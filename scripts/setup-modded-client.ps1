# Finalize the ISOLATED modded SCUM client at F:\SCUM-Modded.
#
# WHY THIS EXISTS (2026-06-16): we used to mod the Steam-managed client in-place —
# hijacking real vanilla pak slots (pakchunk0_s21/s22/s23) + renaming the live BattlEye
# folder. That corrupts the vanilla install every time (Steam keeps no copy of overwritten
# paks), forcing a multi-GB "Verify integrity" re-download just to play vanilla. NEVER AGAIN.
#
# THE MODEL:
#   - VANILLA client = the Steam install (C:\...\common\SCUM). Pristine. Launch via Steam "Play".
#                      Connects to any server (incl. OVH). NEVER touched by mod tooling.
#   - MODDED client  = F:\SCUM-Modded (a full copy on F:, where there's disk room). ALL pak
#                      injection / BattlEye-disable / loader work happens HERE. Launched by
#                      turdmod-launcher (resolve_scum prefers this path) which injects the loader
#                      and bypasses Steam + BattlEye entirely.
#
# Run this AFTER scripts clone the install (robocopy C:\...\SCUM -> F:\SCUM-Modded completes).
# Idempotent.
$ErrorActionPreference = 'Stop'
$mod = 'F:\SCUM-Modded'
$exe = "$mod\SCUM\Binaries\Win64\SCUM.exe"

if (-not (Test-Path $exe)) { throw "Modded client not cloned yet (missing $exe). Wait for the robocopy clone to finish." }
Write-Host "[1/2] Modded client present: $exe"

# Disable BattlEye in the COPY ONLY so the injected loader runs unhindered. The Steam vanilla
# BattlEye is left intact. The launcher starts SCUM.exe directly, so BE never loads here anyway;
# renaming is belt-and-suspenders + documents intent.
$be = "$mod\BattlEye"
if (Test-Path $be) {
  Rename-Item $be "$mod\BattlEye.disabled-by-turdmod" -Force
  Write-Host '[2/2] BattlEye disabled in modded copy (renamed -> BattlEye.disabled-by-turdmod).'
} elseif (Test-Path "$mod\BattlEye.disabled-by-turdmod") {
  Write-Host '[2/2] BattlEye already disabled in modded copy.'
} else {
  Write-Host '[2/2] WARN: no BattlEye folder found in modded copy.'
}

Write-Host ''
Write-Host 'Modded client ready. Launch via turdmod-launcher (it resolves F:\SCUM-Modded first).'
Write-Host 'Custom paks: add them to the MODDED copy only, in UNUSED high slots or as _P patch'
Write-Host 'paks with valid sigs — do NOT hijack vanilla pakchunk0_sNN slots (that is what'
Write-Host 'corrupted the install). Vanilla stays on Steam, untouched.'
