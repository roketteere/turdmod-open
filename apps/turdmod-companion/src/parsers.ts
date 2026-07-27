/**
 * Log-line -> structured event parsers for the SCUM dedicated server.
 *
 * Patterns ported from `scripts/harvest-server-logs.py` — the harvester is
 * the authoritative reference for log shapes (it's been running against
 * Joel's prior local server). Anything that's been verified there should
 * not drift here.
 *
 * Logs are UTF-16 LE without BOM (per overlay commit 7f19112). The tailer
 * decodes them; this module just sees plain JS strings.
 */

export interface Vec3 { x: number; y: number; z: number; }

export type ServerEvent =
  | { kind: "login";    ts: string; ip: string; steam: string; player: string; pos: Vec3 }
  | { kind: "logout";   ts: string; steam: string; player: string }
  | { kind: "kill";     ts: string; victim: string; victimSteam?: string; killer?: string; killerSteam?: string; weapon?: string; distanceM?: number; headshot?: boolean; killerPos?: Vec3; victimPos?: Vec3 }
  | { kind: "vehicle";  ts: string; event: "Destroyed" | "Failed to spawn" | "Disappeared"; vclass: string; vid: string; owner?: string; pos: Vec3 }
  | { kind: "bunker";   ts: string; bid: string; state: "Active" | "Locked"; pos: Vec3 }
  | { kind: "chat";     ts: string; channel: "Local" | "Global" | "Squad"; player: string; steam?: string; text: string }
  | { kind: "admin";    ts: string; steam: string; player: string; cmd: string };

const TS_RE = /^(\d{4}\.\d{2}\.\d{2}-\d{2}\.\d{2}\.\d{2}):\s*(.*)$/;

const LOGIN_RE = /'(?<ip>[^ ]+)\s+(?<steam>\d+):(?<player>[^']+)'\s+logged in at:\s+X=(?<x>-?\d+\.?\d*)\s+Y=(?<y>-?\d+\.?\d*)\s+Z=(?<z>-?\d+\.?\d*)/;
// Logout lines mirror login (with the same `127.0.0.1 ` IP prefix inside the quotes):
//   '127.0.0.1 76561198000000001:SampleAdmin(2)' logged out at: X=... Y=... Z=...
const LOGOUT_RE = /'(?<ip>[^ ]+)\s+(?<steam>\d+):(?<player>[^']+)'\s+logged out/;

const VEHICLE_RE = /\[(?<event>Destroyed|Failed to spawn|Disappeared)\]\s+(?<vclass>[A-Za-z0-9_]+)\.\s+VehicleId:\s+(?<vid>\d+)\.\s+Owner:\s+(?<owner>[^.]+?)\.\s+Location:\s+X=(?<x>-?\d+\.?\d*)\s+Y=(?<y>-?\d+\.?\d*)\s+Z=(?<z>-?\d+\.?\d*)/;

const BUNKER_RE = /\[LogBunkerLock\]\s+(?<bid>[A-Z]\d)\s+Bunker\s+is\s+(?<state>Active|Locked)\..*?X=(?<x>-?\d+\.?\d*)\s+Y=(?<y>-?\d+\.?\d*)\s+Z=(?<z>-?\d+\.?\d*)/;

const ADMIN_RE = /'(?<steam>\d+):(?<player>[^']+)'\s+Command:\s+'(?<cmd>[^']+)'/;

const DIED_RE = /Died:\s+(?<victim>.+?)\s*(?:\(id:\s*(?<victimId>\d+)\))?\s*\.?\s*(?:Killer:\s+(?<killer>.+?)\s*(?:\(id:\s*(?<killerId>\d+)\))?\s*\.?)?/s;
const KILL_WEAPON_RE   = /Weapon:\s*(?<weapon>[^.\n\r]+?)\s*(?:\.|$)/i;
const KILL_DISTANCE_RE = /Distance:\s*(?<dist>-?\d+(?:\.\d+)?)\s*m/i;
const KILL_HEADSHOT_RE = /Headshot:\s*(?<hs>yes|no|true|false)/i;
const KILL_KLOC_RE = /Killer\s+Location:\s*X=(?<x>-?\d+\.?\d*)\s+Y=(?<y>-?\d+\.?\d*)\s+Z=(?<z>-?\d+\.?\d*)/i;
const KILL_VLOC_RE = /Victim\s+Location:\s*X=(?<x>-?\d+\.?\d*)\s+Y=(?<y>-?\d+\.?\d*)\s+Z=(?<z>-?\d+\.?\d*)/i;

// chat.log (actual format confirmed 2026-05-24):
//   '76561198000000001:SampleAdmin(3)' 'Global: !balance'
//   '76561198000000001:SampleAdmin(3)' 'Local: hey nearby'
const CHAT_RE = /'(?<steam>\d+):(?<player>[^']+)'\s+'(?<channel>Local|Global|Squad):\s*(?<text>[^']*)'/;

function vec(g: Record<string, string | undefined>, prefix = ""): Vec3 {
  return {
    x: Number(g[`${prefix}x`] ?? g.x),
    y: Number(g[`${prefix}y`] ?? g.y),
    z: Number(g[`${prefix}z`] ?? g.z),
  };
}

export function splitTs(line: string): { ts: string; rest: string } | null {
  const m = TS_RE.exec(line);
  if (!m) return null;
  return { ts: m[1]!, rest: m[2] || "" };
}

/** Single-line parsers. Returns null if the line isn't a recognized event. */
export function parseSingleLine(line: string): ServerEvent | null {
  const t = splitTs(line);
  if (!t) return null;
  const { ts, rest } = t;

  let m: RegExpExecArray | null;

  m = LOGIN_RE.exec(rest);
  if (m?.groups) {
    return {
      kind: "login", ts,
      ip: m.groups.ip!,
      steam: m.groups.steam!,
      player: m.groups.player!,
      pos: vec(m.groups),
    };
  }

  m = LOGOUT_RE.exec(rest);
  if (m?.groups) {
    return { kind: "logout", ts, steam: m.groups.steam!, player: m.groups.player! };
  }

  m = VEHICLE_RE.exec(rest);
  if (m?.groups) {
    return {
      kind: "vehicle", ts,
      event: m.groups.event as "Destroyed" | "Failed to spawn" | "Disappeared",
      vclass: m.groups.vclass!,
      vid: m.groups.vid!,
      owner: m.groups.owner !== "N/A" ? m.groups.owner : undefined,
      pos: vec(m.groups),
    };
  }

  m = BUNKER_RE.exec(rest);
  if (m?.groups) {
    return {
      kind: "bunker", ts,
      bid: m.groups.bid!,
      state: m.groups.state as "Active" | "Locked",
      pos: vec(m.groups),
    };
  }

  m = ADMIN_RE.exec(rest);
  if (m?.groups) {
    return { kind: "admin", ts, steam: m.groups.steam!, player: m.groups.player!, cmd: m.groups.cmd! };
  }

  m = CHAT_RE.exec(rest);
  if (m?.groups) {
    return {
      kind: "chat", ts,
      channel: m.groups.channel as "Local" | "Global" | "Squad",
      player: m.groups.player!,
      steam: m.groups.steam,
      text: m.groups.text!,
    };
  }

  return null;
}

/**
 * Multi-line kill block parser. Kill events span 2-5 lines; the caller
 * accumulates from a "Died:" line until the next timestamped line, then
 * passes the whole block here.
 */
export function parseKillBlock(block: string): ServerEvent | null {
  const t = splitTs(block.split(/\r?\n/)[0] || "");
  if (!t) return null;
  const ts = t.ts;
  const m = DIED_RE.exec(block);
  if (!m?.groups) return null;
  const evt: ServerEvent = {
    kind: "kill", ts,
    victim: m.groups.victim!.trim(),
    victimSteam: m.groups.victimId,
    killer: m.groups.killer?.trim(),
    killerSteam: m.groups.killerId,
  };

  const w = KILL_WEAPON_RE.exec(block);
  if (w?.groups) (evt as { weapon?: string }).weapon = w.groups.weapon!.trim();
  const d = KILL_DISTANCE_RE.exec(block);
  if (d?.groups) (evt as { distanceM?: number }).distanceM = Number(d.groups.dist);
  const hs = KILL_HEADSHOT_RE.exec(block);
  if (hs?.groups) (evt as { headshot?: boolean }).headshot = /yes|true/i.test(hs.groups.hs!);
  const kl = KILL_KLOC_RE.exec(block);
  if (kl?.groups) (evt as { killerPos?: Vec3 }).killerPos = vec(kl.groups);
  const vl = KILL_VLOC_RE.exec(block);
  if (vl?.groups) (evt as { victimPos?: Vec3 }).victimPos = vec(vl.groups);

  return evt;
}
