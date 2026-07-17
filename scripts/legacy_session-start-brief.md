# SessionStart hook: emit a "where we are" brief as additionalContext
# so future-Claude opens with current state instead of re-deriving.
#
# Output format: JSON with hookSpecificOutput.additionalContext containing
# the markdown brief. Claude Code injects that text into the model context
# at session start.

$ErrorActionPreference = "SilentlyContinue"

# ─── 1. Recent commits on turdmod main ──────────────────────────────────────
$gitDir = "C:\Development\Claude\turdmod"
$commits = "(git not available)"
if (Test-Path "$gitDir\.git") {
    $logOutput = & git -C "$gitDir" log --oneline -3 2>$null
    if ($LASTEXITCODE -eq 0) {
        $commits = ($logOutput -join "`n")
    }
}

# ─── 2. Top entry from IDEAS.md ─────────────────────────────────────────────
$ideasPath = "$gitDir\IDEAS.md"
$ideasTop = "(IDEAS.md not found)"
if (Test-Path $ideasPath) {
    # Grab the first non-front-matter `## YYYY-MM-DD` heading + the next
    # 1-2 lines so the brief shows the actual current direction.
    $lines = Get-Content $ideasPath -Encoding utf8
    $found = $false
    $captured = @()
    foreach ($line in $lines) {
        if (-not $found -and $line -match '^## \d{4}-\d{2}-\d{2}') {
            $found = $true
        }
        if ($found) {
            $captured += $line
            if ($captured.Count -ge 5) { break }
        }
    }
    if ($captured.Count -gt 0) {
        $ideasTop = ($captured -join "`n")
    }
}

# ─── 3. SCUMServer state ─────────────────────────────────────────────────────
$scumState = "(could not check)"
$tasklist = & tasklist /FI "IMAGENAME eq GameServer.exe" /NH 2>$null
if ($tasklist -match "SCUMServer\.exe") {
    $scumState = "RUNNING (alive)"
} else {
    $scumState = "stopped"
}

# ─── 4. TurdMOD-specific custom skills ──────────────────────────────────────
$skills = @()
$skillsDir = "$HOME\.claude\skills"
if (Test-Path $skillsDir) {
    foreach ($d in Get-ChildItem $skillsDir -Directory) {
        $skillFile = Join-Path $d.FullName "SKILL.md"
        if (Test-Path $skillFile) {
            # Pull the description line from the frontmatter.
            $content = Get-Content $skillFile -Encoding utf8 -TotalCount 10
            $desc = ($content | Where-Object { $_ -match '^description:' } | Select-Object -First 1) -replace '^description:\s*', ''
            if (-not $desc) { $desc = "(no description)" }
            # Trim to ~140 chars so the brief stays compact
            if ($desc.Length -gt 140) { $desc = $desc.Substring(0, 137) + "..." }
            $skills += "  - $($d.Name): $desc"
        }
    }
}
$skillList = if ($skills.Count -gt 0) { $skills -join "`n" } else { "  (no custom skills found)" }

# ─── Compose the brief ───────────────────────────────────────────────────────
$brief = @"
## TurdMOD session-start brief

**Latest commits (turdmod main):**
``````
$commits
``````

**Top IDEAS.md entry:**
``````
$ideasTop
``````

**GameServer.exe state:** $scumState

**Custom skills available (~/.claude/skills/):**
$skillList

(Auto-injected by ~/.claude/settings.json SessionStart hook. Source: scripts/session-start-brief.ps1)
"@

# Output as JSON for Claude Code's hook contract.
$payload = @{
    hookSpecificOutput = @{
        hookEventName = "SessionStart"
        additionalContext = $brief
    }
} | ConvertTo-Json -Depth 5 -Compress

Write-Output $payload
