import { useCallback, useEffect, useRef, useState } from 'react';
import { engineRpc } from '../lib/tauri-engine';

// First real "TurdMOD mod" — auto-sends a chat line to a player the moment
// they join. Runs entirely on the Manager side: polls getOnlinePlayers every
// few seconds, diffs the roster, and fires sendChatLineToPlayer for each new
// name. Config is persisted in localStorage so it survives Manager restarts.
//
// The hook must be mounted at App level (not inside a page) so polling
// continues regardless of which tab the user is on.

const STORAGE_KEY = 'turdmod.welcomeMod.v1';
const POLL_INTERVAL_MS = 5000;

export const CHANNEL_OPTIONS = ['Local', 'Squad', 'Global', 'Admin'] as const;
export type WelcomeChannel = (typeof CHANNEL_OPTIONS)[number];

export interface WelcomeModConfig {
  enabled: boolean;
  message: string;
  channel: WelcomeChannel;
}

export interface WelcomeModStatus {
  lastFiredPlayer: string | null;
  lastFiredAt: Date | null;
  totalFiredCount: number;
  lastError: string | null;
}

interface PlayerEntry {
  name: string;
  ptr: string;
  controller: string;
  class: string;
}

interface GetOnlinePlayersResponse {
  count: number;
  players: PlayerEntry[];
}

const DEFAULTS: WelcomeModConfig = {
  enabled: false,
  message: 'Welcome to the server, {playerName}!',
  channel: 'Global',
};

function loadConfig(): WelcomeModConfig {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULTS };
    const parsed = JSON.parse(raw) as Partial<WelcomeModConfig>;
    return {
      enabled: typeof parsed.enabled === 'boolean' ? parsed.enabled : DEFAULTS.enabled,
      message: typeof parsed.message === 'string' ? parsed.message : DEFAULTS.message,
      channel: CHANNEL_OPTIONS.includes(parsed.channel as WelcomeChannel)
        ? (parsed.channel as WelcomeChannel)
        : DEFAULTS.channel,
    };
  } catch {
    return { ...DEFAULTS };
  }
}

function saveConfig(cfg: WelcomeModConfig): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(cfg));
  } catch {
    // Quota exceeded / disabled — silently skip; in-memory state still works.
  }
}

export function useWelcomeMod(): {
  config: WelcomeModConfig;
  setConfig: (update: Partial<WelcomeModConfig>) => void;
  status: WelcomeModStatus;
} {
  const [config, setConfigState] = useState<WelcomeModConfig>(loadConfig);
  const [status, setStatus] = useState<WelcomeModStatus>({
    lastFiredPlayer: null,
    lastFiredAt: null,
    totalFiredCount: 0,
    lastError: null,
  });

  // null = no baseline yet. First successful poll establishes it without
  // firing welcomes (so we don't bulk-greet everyone who was already online).
  const baselineRef = useRef<Set<string> | null>(null);
  const configRef = useRef(config);
  configRef.current = config;

  const setConfig = useCallback((update: Partial<WelcomeModConfig>) => {
    setConfigState((prev) => {
      const next = { ...prev, ...update };
      saveConfig(next);
      return next;
    });
  }, []);

  useEffect(() => {
    if (!config.enabled) {
      // Reset baseline so re-enabling later doesn't fire welcomes for
      // everyone already online at that moment.
      baselineRef.current = null;
      return;
    }

    let cancelled = false;

    const tick = async () => {
      if (cancelled) return;
      const cfg = configRef.current;

      let players: PlayerEntry[];
      try {
        const res = await engineRpc<GetOnlinePlayersResponse>('getOnlinePlayers');
        players = res.players ?? [];
      } catch {
        // Engine off / pipe unavailable — reset baseline so we don't fire a
        // wave when it comes back.
        baselineRef.current = null;
        return;
      }
      if (cancelled) return;

      const currentNames = new Set(players.map((p) => p.name).filter(Boolean));

      if (baselineRef.current === null) {
        baselineRef.current = currentNames;
        return;
      }

      const message = cfg.message.trim();
      if (!message) {
        baselineRef.current = currentNames;
        return;
      }

      const newPlayers = [...currentNames].filter((n) => !baselineRef.current!.has(n));
      baselineRef.current = currentNames;

      for (const playerName of newPlayers) {
        if (cancelled) break;
        const interpolated = message.replace(/\{playerName\}/g, playerName);
        try {
          await engineRpc('sendChatLineToPlayer', {
            playerName,
            message: interpolated,
            channel: cfg.channel,
          });
          const now = new Date();
          setStatus((prev) => ({
            lastFiredPlayer: playerName,
            lastFiredAt: now,
            totalFiredCount: prev.totalFiredCount + 1,
            lastError: null,
          }));
        } catch (err) {
          setStatus((prev) => ({
            ...prev,
            lastError: String((err as Error)?.message ?? err),
          }));
        }
      }
    };

    tick();
    const id = setInterval(tick, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [config.enabled]);

  return { config, setConfig, status };
}
