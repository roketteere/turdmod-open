# ollama-pool — multi-endpoint LLM dispatcher (CLI side)

CLI counterpart of the Manager's Ollama Bridge (Tools → Ollama Bridge).
Same endpoint registry shape; usable directly from PowerShell.

> **Status:** experimental subsystem inside turdmod, slated for extraction
> into a standalone Apache-2.0 app named **TeKi Bridge** in a future
> session. Don't build anything load-bearing on these script paths yet.

## Files

| File | Purpose |
|---|---|
| `endpoints.json` | Endpoint registry — name, URL, tier (`local`/`lan`/`cloud`), tags |
| `health.ps1` | Pings every endpoint's `/api/tags` + `/api/version`, returns JSON status |
| `dispatch.ps1` | Sends a prompt to a chosen endpoint, returns generated text + perf stats |

## Quick usage

```powershell
# Are all endpoints alive?
.\health.ps1 | ConvertFrom-Json | Select -ExpandProperty endpoints | Format-Table name, online, latency_ms

# Send a prompt to a specific endpoint
.\dispatch.ps1 -Endpoint wife-rig -Prompt "Write one line of Rust that squares an int" -Model qwen2.5-coder:7b

# Tune output length + temperature
.\dispatch.ps1 -Endpoint local -Prompt "..." -NumPredict 256 -Temperature 0.1
```

Output is a single-line JSON object: `endpoint`, `model`, `response`,
`eval_count`, `tokens_per_sec`, `total_duration_ms`, `error`.

## Endpoint schema

```json
{
  "name": "wife-rig",
  "url": "http://YOUR_LAN_IP:11434",
  "tier": "lan",
  "host": "wife-desktop",
  "cost_per_hr": 0,
  "preferred_models": ["qwen2.5-coder:7b"],
  "tags": ["free", "second-brain"]
}
```

Add new endpoints by appending to `endpoints.json` and they show up
in both CLI scripts and the Manager GUI on next read.

## Relationship to the Manager GUI

The Manager's `Tools → Ollama Bridge` page uses its own registry at
`%LOCALAPPDATA%\TurdMOD\ollama-endpoints.json` (auto-materialized on
first load). These CLI scripts use the in-repo `endpoints.json`. The
schemas match; keeping them in sync is currently manual. After the
TeKi Bridge extraction the two will share one canonical registry.
