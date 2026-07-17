// Two technical tiers, as defined in docs/architecture.md.
//
// - "lite": managed hosts (G-Portal, Nitrado, etc.) where `Binaries/` is
//   off-limits. Operate via FTP for config and RCON for live admin.
//   ~70% of SCUM's practical mod surface.
//
// - "engine": own-the-binaries hosts (VPS / dedicated) where UE4SS +
//   TurdMODEngineBridge.dll are loaded in-process. Operate via a named
//   pipe into the engine bridge. ~100% of the mod surface.
//
// Lite is a strict subset of Engine — anything an EngineAdapter can do,
// it can do via the same FTP+RCON path Lite uses. Engine adds engineRpc
// for direct UFunction calls and an event stream subscription.
export type ServerTier = 'lite' | 'engine';

// Each adapter exposes one of these to the UI so feature gates can
// decide what to show. Lite-tier pages hide engine-only controls.
export interface TierCapabilities {
  readonly tier: ServerTier;
  readonly canReadFiles: boolean;
  readonly canWriteFiles: boolean;
  readonly canRcon: boolean;
  readonly canEngineRpc: boolean;
  readonly canSubscribeEvents: boolean;
}

export const LITE_CAPABILITIES: TierCapabilities = {
  tier: 'lite',
  canReadFiles: true,
  canWriteFiles: true,
  canRcon: true,
  canEngineRpc: false,
  canSubscribeEvents: false,
};

export const ENGINE_CAPABILITIES: TierCapabilities = {
  tier: 'engine',
  canReadFiles: true,
  canWriteFiles: true,
  canRcon: true,
  canEngineRpc: true,
  canSubscribeEvents: true,
};
