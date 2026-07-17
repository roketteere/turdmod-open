```markdown
# SCUM Server Optimization Suite — Specification

**Version**: 1.0  
**Target**: SCUMServer.exe (UE 4.27) running UE4SS with PolyHook2  
**Author**: turdmod senior performance engineer  
**Status**: Draft for implementation  

---

## 1. Observable Surface — Every UE 4.27 Server-Side Telemetry Source

The bridge will tap the following sources, each with exact code path, global, or class reference. All hooks are implemented via PolyHook2 detours; reflective access uses UE4SS’s `UKismetSystemLibrary` and raw memory reads where needed.

### 1.1 Frame Timing & FPS
- **`GFrameCounter`** — `extern ENGINE_API uint64 GFrameCounter;` (declared in `Runtime/Engine/Public/UnrealEngine.h`). Incremented each tick.
- **`GAverageFPS`** — Not available by default in shipping builds. We compute our own moving average from `FApp::GetCurrentTime()`.  
- **`FApp::GetCurrentTime()`** — Returns `double` (seconds since startup). Header: `Runtime/Core/Public/HAL/PlatformTime.h`.  
- **`FPlatformTime::Cycles64()`** — Returns `uint64` CPU cycles. Use for high-precision delta measurements.  
- **Tick per-world**: `GEngine->GetWorldContexts()` (returns `TArray<FWorldContext>`). For each `FWorldContext::World()` we can hook `UWorld::Tick(ELevelTick TickType, float DeltaSeconds)`. PolyHook2 detour on that function.  

### 1.2 Object & Actor Counts
- **UObject count**: `GUObjectArray.GetObjectArrayNum()` — `GUObjectArray` is a `FChunkedFixedUObjectArray` global, declared in `Runtime/CoreUObject/Public/UObject/UObjectArray.h`. Access via `FUObjectArray& GUObjectArray = GetUObjectArray()`.  
- **Actor count**: Walk `UWorld::PersistentLevel->Actors` array. Each `AActor*` in the `TArray<AActor*>`.  
- **Player count**: `UWorld::GetGameInstance()->GetGameMode()->NumPlayers` (or iterate `GameState->PlayerArray`).

### 1.3 Memory Statistics
- **`FPlatformMemory::GetStats()`** — Returns `FPlatformMemoryStats`. Header: `Runtime/Core/Public/Windows/WindowsPlatformMemory.h`. Fields of interest: `UsedPhysical` (bytes), `AvailablePhysical`, `PeakUsedPhysical`.  
- **Process memory**: Windows API `GetProcessMemoryInfo` (PSAPI) for `PROCESS_MEMORY_COUNTERS`, `PagefileUsage`, `PeakPagefileUsage`.  
- **UE allocator pools**: If compiled with `USE_POOL_ALLOCATOR`, access `FMalloc* GMalloc` from `ModuleManager.h`; use `GMalloc->GetAllocatorStats()`.

### 1.4 Garbage Collection
- **`GetUObjectArray().GetObjectArrayCapacity()`** — current capacity.  
- **`GExitPurge`** — global bool in `UnrealEngine.cpp`.  
- **Hook `CollectGarbage(EGarbageCollectionFlags, bool bPerformFullPurge)`** — function in `Runtime/CoreUObject/Public/UObject/UObjectGlobals.h`. PolyHook2 measure entry/exit time, capture `GUObjectArray` count before/after.  
- **Last GC time**: Store timestamp at entry (using `FPlatformTime::Seconds()`).

### 1.5 Replication & Network
- **NetDriver**: `UWorld::GetNetDriver()` returns `UNetDriver*`.  
- **Client connections**: `UNetDriver::ClientConnections` — `TArray<UNetConnection*>`.  
- **SendBunch hook**: `bool UNetDriver::SendBunch(UNetConnection*, FOutBunch&, uint8*)`. PolyHook2 to capture bytes per second per connection.  
- **ReceivedRawPacket**: `void UNetConnection::ReceivedRawPacket(void* Data, int32 Count)`. Hook for incoming bandwidth.  
- **Bandwidth stats**: `FNetworkProfiler` if built with `NETWORK_PROFILER` flag. Check `#if NETWORK_PROFILER`; we can also compute manually from hooked send/receive size sums.

### 1.6 Tick Budget & Systems
- **Hook `UWorld::Tick()`** — PolyHook2 on `UWorld::Tick(ELevelTick, float)`. Measure duration via rdtsc, store in ring buffer per-world.  
- **STAT system**: If server built with `-STATS` flag, we can read `FStatsThread` and access cycle counters via `FStatPacket`. Check at runtime: `FStatGroupManager::GetStatGroupManager().Get...` (complex). For simplicity we compute our own per-system costs by hooking specific tick functions (e.g., `APlayerController::ProcessPlayerInput`, `AAIController::Tick`).  

### 1.7 Disk I/O
- **Windows API**: `GetProcessIoCounters(GetCurrentProcess(), &IO_COUNTERS)`. Provides `ReadOperationCount`, `WriteOperationCount`, `ReadTransferCount`, `WriteTransferCount`.  

### 1.8 Thread CPU Usage
- **`GetProcessTimes`**: Kernel and user time for whole process.  
- **Per-thread**: Enumerate threads with `CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)`, then `OpenThread` + `GetThreadTimes` for each. Cache handles, refresh every 10 seconds.  

### 1.9 Pak File Info
- `FPakFile` list from `FPakPlatformFile` (S_`Mount`). Access via `FCoreDelegates::GetPakFileRegisteredDelegates()`? Simpler: read the file `../Content/Paks/*.pak` and parse header. Use `IPlatformFile::FindFiles()`.

### 1.10 Detour Infrastructure
All hooks use **PolyHook2**:  
```cpp
#include <polyhook2/Detour/x64Detour.hpp>  
// detour address: GEngine->WorldTick, etc.  
// Resolve address via UE4SS reflection (UFunction::GetNativeFunc()) or pattern scan.  
```

---

## 2. Bridge Handlers — JSON Contract

Each handler is a JSON-RPC method invoked by turdmod-companion over the named pipe. Request format: `{"jsonrpc":"2.0","method":"<method>","params":{},"id":1}`. Response: `{"jsonrpc":"2.0","result":<object>,"id":1}`. Error: `{"jsonrpc":"2.0","error":{"code":-1,"message":"<reason>"},"id":1}`.

### 2.1 `getServerStats`
**Params**: none  
**Response**:  
```json
{
  "frameNumber": 12345678,
  "fps": 29.97,
  "tickMs": 33.4,
  "usedPhysicalMB": 4096,
  "availablePhysicalMB": 8192,
  "playerCount": 64,
  "actorCount": 15000,
  "uobjectCount": 1200000,
  "gcCount": 42,
  "lastGCMs": 5.3,
  "netBandwidthBytesPerSec": 2500000,
  "diskReadBytesPerSec": 100000,
  "diskWriteBytesPerSec": 50000
}
```

### 2.2 `getReplicationStats`
**Params**: `{"classFilter": "BP_Zombie_C"}` (optional)  
**Response**:  
```json
{
  "totalBandwidthBytesPerSec": 2500000,
  "classes": [
    {
      "className": "BP_Zombie_C",
      "instanceCount": 500,
      "avgReplicationBitsPerSec": 80000,
      "cullDistance": 15000
    }
  ],
  "connections": [
    {"playerId": 1, "bytesSentPerSec": 40000, "bytesRecvPerSec": 2000, "packetLoss": 0.001, "rttMs": 45}
  ]
}
```

### 2.3 `getMemoryProfile`
**Params**: none  
**Response**:  
```json
{
  "processUsedMB": 4096,
  "peakUsedMB": 5120,
  "availableMB": 8192,
  "poolAllocatorSizeMB": 1024,
  "texturePoolMB": 512,
  "renderThreadAllocMB": 128
}
```
*(Need to extract pool stats from FMalloc; if not compiled with pool, omit. Also read `FTexturePool` via `GTexturePool` if visible)*

### 2.4 `getTickProfile`
**Params**: none  
**Response**:  
```json
{
  "totalTickMs": 33.4,
  "systems": [
    {"name": "PrePhysics", "ms": 5.2},
    {"name": "Physics", "ms": 8.1},
    {"name": "PostPhysics", "ms": 3.3},
    {"name": "UpdateCamera", "ms": 1.0},
    {"name": "PlayerTick", "ms": 4.5},
    {"name": "AITick", "ms": 6.0},
    {"name": "NetDriverTick", "ms": 2.0},
    {"name": "GC", "ms": 0.5}
  ]
}
```
*Capture durations by hooking specific tick phases. We'll use `UWorld::Tick` with phases and subtick hooks.*

### 2.5 `getGCStats`
**Params**: none  
**Response**:  
```json
{
  "totalGCCount": 42,
  "lastGCTimestamp": 1623456789.123,
  "lastGCDurationMs": 5.3,
  "objectsBefore": 1200000,
  "objectsAfter": 1199500,
  "averageIntervalMs": 1200,
  "averageDurationMs": 4.8
}
```

### 2.6 `getActorPopulation`
**Params**: `{"class": "BP_Zombie_C"}` (optional)  
**Response**:  
```json
{
  "totalActorCount": 15000,
  "byClass": [
    {"className": "BP_Zombie_C", "count": 500, "avgDistanceFromOrigin": 1200},
    {"className": "Character", "count": 64, "avgDistanceFromOrigin": 300}
  ]
}
```

### 2.7 `getNetworkStats`
**Params**: none  
**Response**:  
```json
{
  "connections": [
    {"playerId": 1, "bytesSentPerSec": 40000, "bytesRecvPerSec": 2000, "packetLoss": 0.001, "rttMs": 45},
    {"playerId": 2, ...}
  ],
  "totalBytesPerSec": 2500000,
  "peakBytesPerSec": 3000000
}
```

### 2.8 `getDiskIO`
**Params**: none  
**Response**:  
```json
{
  "readOperations": 1000,
  "writeOperations": 200,
  "readBytesTotal": 1073741824,
  "writeBytesTotal": 536870912,
  "readBytesPerSec": 100000,
  "writeBytesPerSec": 50000
}
```

### 2.9 `getThreadProfile`
**Params**: none  
**Response**:  
```json
{
  "processUserMs": 30000,
  "processKernelMs": 15000,
  "threads": [
    {"tid": 1234, "name": "GameThread", "cpuPercent": 45.2, "userMs": 20000},
    {"tid": 5678, "name": "RenderThread", "cpuPercent": 30.1, "userMs": 10000},
    {"tid": 9012, "name": "WorkerThread0", "cpuPercent": 5.0, "userMs": 2000}
  ]
}
```
*Thread names obtained from UE4 `FThreadManager::Get().GetThreadName(FThreadId)*`; if unavailable, fallback to `GetThreadDescription` Windows 10+.*

### 2.10 `getPakStats`
**Params**: none  
**Response**:  
```json
{
  "paks": [
    {"filename": "Game.pak", "sizeMB": 2048, "mountPoint": "../../../Game"},
    {"filename": "Engine.pak", "sizeMB": 512, "mountPoint": "../../../Engine"}
  ],
  "totalSizeMB": 2560
}
```

### 2.11 `getHotspots`
**Params**: `{"type": "memory"}` (also "tickTime", "replicationBandwidth")  
**Response**:  
```json
{
  "hotspots": [
    {"class": "BP_Zombie_C", "metric": 500, "unit": "MB"},
    {"class": "Building", "metric": 300, "unit": "MB"}
  ]
}
```
*`type` determines sorting: memory = sum of actor memory (crude: each actor size guessed from class default object; more accurate via `GetResourceSizeEx`), tickTime = cumulative tick ms, replicationBandwidth = bytes/sec.*

### 2.12 `forceGC`
**Params**: `{}`  
**Response**: `{"result": "ok", "gcTriggered": true}`  
*Calls `CollectGarbage(GARBAGE_COLLECTION_KEEPFLAGS, true)` via `UGameplayStatics::ForceCollectGarbage` or direct detour.*

### 2.13 `setMaxTickRate`
**Params**: `{"hz": 30}`  
**Response**: `{"result": "ok", "currentFps": 30.0}`  
*Sets `GEngine->SetMaxFPS(30.0f)` or modify `GEngine->MaxFPS` directly.*

### 2.14 `flushUnusedAssets`
**Params**: `{}`  
**Response**: `{"result": "ok"}`  
*Calls `CleanupUnusedAssets()` via `UEngine` or manually iterate `FStreamableManager`.

### 2.15 `setReplicationCullDistance`
**Params**: `{"class": "BP_Zombie_C", "distance": 10000}`  
**Response**: `{"result": "ok"}`  
*Loops all instances of class in world and sets `AActor::NetCullDistanceSquared` (square of distance).*

### 2.16 `setAIBudget`
**Params**: `{"maxMs": 5.0}`  
**Response**: `{"result": "ok"}`  
*Hook `AAIController::Tick` and early return if cumulative AI tick time in frame exceeds limit. Store budget in global variable.*

---

## 3. Event Firehose — Push Events

The bridge will send unsolicited JSON-RPC notifications (id: null) to the companion when thresholds are breached.

### 3.1 `serverTick`
**Frequency**: configurable (default 1 Hz)  
**Params**: same as `getServerStats` response.  
Trigger: periodic timer.

### 3.2 `lowFrameRate`
**Trigger**: tick time > `warnTickMsThreshold` (default 50ms → 20 FPS).  
**Params**: `{"tickMs": 55.0, "fps": 18.2}`

### 3.3 `memoryPressure`
**Trigger**: `usedPhysicalMB > maxMemoryMB` (default 7000) OR growth rate > 100 MB in last 10 seconds.  
**Params**: `{"usedPhysicalMB": 7200, "growthMBperSec": 20}`

### 3.4 `gcStall`
**Trigger**: GC pause > `gcStallMsThreshold` (default 30ms).  
**Params**: `{"gcDurationMs": 35.0}`

### 3.5 `replicationStorm`
**Trigger**: bandwidth per connection > 100 KB/s or total > 10 MB/s.  
**Params**: `{"totalBytesPerSec": 12500000}`

### 3.6 `actorExplosion`
**Trigger**: class count increased more than 20% in 30 seconds.  
**Params**: `{"className": "BP_Zombie_C", "countBefore": 500, "countNow": 650, "timeSpanSec": 30}`

---

## 4. Auto-Tuner Architecture (ODAV Loop)

### 4.1 Observation
- Continuous telemetry capture every N ms (default 100ms) into a ring buffer with 1-hour capacity (36,000 entries per metric).  
- Stored in shared memory (process-local) for fast access.  
- Implemented as a separate worker thread with high-frequency timers (using `CreateTimerQueueTimer`).

### 4.2 Diagnosis
- Rule engine evaluates conditions against trailing windows (30s, 5min, 1h).  
- Example rules:
  - `IF (fps < 20 for 30s) AND (actorCount > 15000) THEN action = "reduce zombie spawn rate"`  
  - `IF (memoryGrowthRate > 50MB/10s) AND (GC interval > 2s) THEN action = "force GC"`  
  - `IF (netBandwidth > 5MB/s) THEN action = "tighten replication cull distance by 10%"`

### 4.3 Decision
- Scored options: each action has a predicted impact (from historical data) and a risk level (e.g., 0–1).  
- Choose action with highest impact/risk ratio.  
- Risk factors: action that reduces player count or changes net cull distances are high risk; forcing GC is low risk.

### 4.4 Action
- Execute via bridge handler call (e.g., `setReplicationCullDistance`).  
- Log to file `auto_tuner.log`.

### 4.5 Verification
- After 60 seconds, compare current telemetry to pre-action baseline (captured 60s before action).  
- Compute improvement ratio: `(old_metric - new_metric)/old_metric`. If positive, mark successful.

### 4.6 Rollback
- If improvement ratio < 0.05 (or negative), revert action (restore previous value).  
- Rollback stored state in a stack (max depth 3).  
- If rollback fails (e.g., same values), increment error counter and stop auto-tuner until manual reset.

---

## 5. `scumpilot` CLI Subcommand `perf`

All commands communicate with the bridge via named pipe.

### 5.1 `perf snapshot`
- **Description**: Single-shot `getServerStats` and print formatted.  
- **Output**:
  ```
  Server Performance Snapshot (timestamp):  
  FPS: 29.97 | Tick: 33.4 ms | Players: 64 | Actors: 15000  
  Memory: 4096 MB (physical) / 8192 MB available  
  GC: 42 collections, last 5.3 ms  
  Network: 2.5 MB/s | Disk R: 100 KB/s W: 50 KB/s  
  ```

### 5.2 `perf watch [--hz N]`
- **Description**: Continuous live dashboard with optional frequency (default 1 Hz).  
- **Implementation**: Calls `getServerStats` every 1/N seconds, clears terminal and prints updated grid.  
- **Output** (example, with color on supported terminals):
  ```
  ┌─────────────────┬──────────┐  
  │ Metric          │ Value    │  
  ├─────────────────┼──────────┤  
  │ FPS             │ 29.97    │  
  │ Tick ms         │ 33.4     │  
  │ Players         │ 64       │  
  │ Actors          │ 15000    │  
  │ Memory          │ 4.0 GB   │  
  │ GC last         │ 5.3 ms   │  
  │ Net (total)     │ 2.5 MB/s │  
  └─────────────────┴──────────┘  
  ```

### 5.3 `perf record <duration> --out file.jsonl`
- **Description**: Poll `getServerStats` at 1 Hz for `duration` seconds (max 3600), write JSON objects line-by-line to `file.jsonl`.  
- **Usage**: `scumpilot perf record 60 --out /tmp/performance.jsonl`

### 5.4 `perf report <file.jsonl>`
- **Description**: Analyze JSONL file and produce summary with recommendations.  
- **Output**:  
  ```
  Report for performance.jsonl:
  - Duration: 60 seconds
  - Average FPS: 29.5, Min: 15.2 at 12:34:56
  - Memory trend: stable at ~4 GB
  - GC events: 3 stalls > 30 ms (see timestamps)
  - Network: peak 3.5 MB/s at 12:35:00
  - Recommendations:
    * Reduce zombie spawn rate during peak (actors > 15000)
    * Increase GC frequency during memory pressure
  ```

### 5.5 `perf hotspots`
- **Description**: Calls `getHotspots` for memory, tickTime, replicationBandwidth and prints top 10.  
- **Output**:  
  ```
  Top Memory Consumers:
  1. BP_Zombie_C: 500 MB (500 instances)
  2. Building: 300 MB (300 instances)
  ...
  ```

### 5.6 `perf tune [--auto|--dry-run]`
- **Description**: Activate or test the auto-tuner.  
- `--auto`: enables continuous auto-tuning (may conflict with manual tweaks).  
- `--dry-run`: diagnoses and prints recommended actions without applying.  
- **Output**: For dry-run, prints list of actions and estimated impact.

---

## 6. Long-Running Aggregation

### 6.1 Data Storage
- **Primary**: NDJSON files (newline-delimited JSON) written to a directory (e.g., `turdmod/perf-logs/`).  
- **Per-file**: One file per hour, named `perf-YYYY-MM-DD-HH.jsonl`.  
- **Rotation**: Old files older than 7 days are deleted. Kept in a rolling window.

### 6.2 Companion App Integration (turdmod-manager)
- The companion spawns a `turdmod-perf-monitor` subprocess that reads the NDJSON files and serves a REST API for historical queries.  
- **Query Interface**:  
  - `GET /api/perf/query?from=<ISO>&to=<ISO>&metrics=fps,memory` returns aggregated time series.  
  - `GET /api/perf/hotspots?from=<...>&window=1h` returns top hotspots per period.  
- **Web UI**: A page in turdmod-manager with plotly graphs showing trends, hotspots, and auto-tuner actions.

### 6.3 Web UI Integration
- Tab "Performance" in turdmod-manager.  
- Live dashboard: server push via WebSocket (reusing event firehose).  
- Historical: date range picker, show line charts for FPS, memory, network, GC times.  
- Hotspots table: sortable by class and metric.  
- Auto-tuner status: enable/disable toggle, recent actions log.

---

## 7. Shipping Wave Plan

### Wave A (2 hours): Core Stats & CLI Snapshot
- Implement `getServerStats` handler with FPS, tick time, memory, player count, actor count, UObject count.  
- Implement `perf snapshot` command.  
- Add PolyHook2 detour on `UWorld::Tick` for tick timing.  
- Add `FPlatformMemory::GetStats` read.  
- **Deliverable**: `perf snapshot` output human-readable.

### Wave B (4 hours): Full Handlers + `perf watch`
- All remaining handlers: replication, memory profile, tick profile, GC, actor population, network, disk, thread, pak, hotspots.  
- `perf watch` with live stats.  
- `perf record` and `perf report` basic (only summary stats).  
- Event firehose with configurable thresholds.

### Wave C (6 hours): Event Firehose & Alerts
- Implement push events for all triggers.  
- Add auto-tuner observation + diagnosis (rule engine).  
- Integrate event streaming to companion.

### Wave D (8 hours): Auto-Tuner v1
- Complete ODAV loop with action execution, verification, rollback.  
- Add `perf tune --auto/--dry-run`.  
- Test on live server to prevent regressions.

### Wave E (future): ML-Based Predictor
- Move to next session after collecting 2+ weeks of historical data.  
- Use simple linear regression or LSTM to predict FPS, memory 5 minutes ahead.  
- Preemptive actions (e.g., throttle spawn rate before predicted spike).  
- Gated on data availability.

---

## 8. Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| **Sampling overhead** – `GetProcessMemoryInfo` costs ~1μs, but called 10×/sec adds 10μs/sec (negligible). Heavy operations like iterating `GUObjectArray` (1.5M objects) may take 1–2 ms – cache results and only refresh every 10 seconds. | Use separate low-priority thread with adaptive sampling rate based on server load. |
| **Replication graph locking** – reading `UNetDriver::ClientConnections` may require lock; can stall network thread. | Clone connection stats via atomic swap (lock-free structure) or take snapshot on network thread itself. |
| **Auto-tuner degrading server** – aggressive actions could reduce player count or break replication. | Default dry-run mode; require admin consent to enable. Rollback ensures safety. |
| **Missing STATS build flag** – many UE intrinsic cycle counters may be absent. | Fall back to our own instrumentation (PolyHook2 detours). |
| **PolyHook2 incompatibility** – future UE4SS updates may break hooks. | Wrap hooks with version checks; use robust pattern scanning for addresses. |

---

## 9. The ‘GOD Level’ Differentiator

- **UE Internal Telemetry**: We expose counters that AAA studios use during development but are hidden in shipped games – e.g., per-system tick costs, object pool stats.  
- **Closed-Loop Auto-Tuning**: No other SCUM mod has a feedback loop that applies – measures – rolls back.  
- **Per-Player Session Profiling**: Hook each `UNetConnection` to attribute network and CPU usage to specific players. Identify which players cause server spikes (e.g., one player with 1000 zombies).  
- **Predictive Analytics**: Use historical data to forecast server load and preemptively adjust settings.  
- **Historic Regression Detection**: Compare week-over-week metrics; alert if baseline degrades (e.g., memory leak, actor population slowly climbs).  

This suite will give SCUM server administrators unprecedented insight and control, outclassing even first-party tools from major studios.

---

## Appendices

### A. PolyHook2 Detour Example
```cpp
#include <polyhook2/Detour/x64Detour.hpp>

// Original function pointer
typedef void (*UWorldTickFn)(UWorld*, ELevelTick, float);
UWorldTickFn OriginalUWorldTick;

void HookedUWorldTick(UWorld* World, ELevelTick TickType, float DeltaSeconds) {
    uint64 StartCycles = FPlatformTime::Cycles64();
    OriginalUWorldTick(World, TickType, DeltaSeconds);
    uint64 EndCycles = FPlatformTime::Cycles64();
    // Store duration in ring buffer
    g_TickDurations[World] = FPlatformTime::ToMilliseconds64(EndCycles - StartCycles);
}

// Installation (on mod init)
void InstallHooks() {
    // Get UWorld::Tick function address via reflection
    UFunction* TickFn = UWorld::StaticClass()->FindFunctionByName(FName("Tick"));
    void* TickAddr = TickFn->GetNativeFunc();
    PLH::x64Detour* detour = new PLH::x64Detour((char*)TickAddr, (char*)&HookedUWorldTick, (uint64_t*)&OriginalUWorldTick);
    detour->hook();
}
```

### B. Windows API: GetProcessIoCounters
```cpp
IO_COUNTERS ioCounters;
if (GetProcessIoCounters(GetCurrentProcess(), &ioCounters)) {
    uint64 readBytes = ioCounters.ReadTransferCount;
    uint64 writeBytes = ioCounters.WriteTransferCount;
}
```

### C. JSON-RPC Connection
Named pipe path: `\\.\pipe\turdmod_bridge_<processid>`. Use `CreateFile`, `WriteFile`, `ReadFile` with `OVERLAPPED` for async.

---
```