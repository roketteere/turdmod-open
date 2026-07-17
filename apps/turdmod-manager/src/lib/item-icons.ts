// Game-asset icon resolver. Maps a SCUM item / loot leaf / tradeable code (e.g. "1H_Cleaver",
// "Pot1", "Scum_Sunrise", "Weapon_AK47") to its extracted icon under /item-icons/ (ico_*.png +
// _index.json, build 23623039). The icon FILENAMES are item codes (ICO_<item>); loot/tradeable
// names are the same codes but often carry size prefixes (1H_/2H_) and trailing variant numbers
// the icons don't. So: exact match -> loosened match (drop size prefix + trailing digits) ->
// best-substring fuzzy (longest specific overlap). onError-fallback still recommended at the call site.

// Base normalize: lowercase, drop ICO_/weapon/item prefixes + display-variant suffixes, strip
// non-alphanumerics. Used for BOTH icon-index keys and the query so they compare apples-to-apples.
function norm(s: string): string {
  return s
    .toLowerCase()
    .replace(/^ico_/, '')
    .replace(/^(weapon|item|bpc|bp)_/, '')
    .replace(/_(inhands|in_hands|inventory|inv|vicinity|lighted)$/g, '')
    .replace(/[^a-z0-9]/g, '');
}

// Loosen a query key: drop a leading 1h/2h size prefix and a trailing variant number
// ("1hcleaver"->"cleaver", "pot1"->"pot", "kitchenboard01"->"kitchenboard").
function loosen(k: string): string {
  return k.replace(/^[12]h/, '').replace(/\d+$/, '');
}

let cache: Promise<Map<string, string>> | null = null;

export function loadIconMap(): Promise<Map<string, string>> {
  if (cache) return cache;
  cache = fetch('/item-icons/_index.json')
    .then((r) => r.json())
    .then((arr: Array<{ exportName?: string; file?: string }>) => {
      // Prefer the inventory variant, then in-hands, then base, then vicinity.
      const rank = (f: string) =>
        /inventory/.test(f) ? 0 : /inhands|in_hands/.test(f) ? 1 : /vicinity/.test(f) ? 3 : 2;
      const best = new Map<string, { url: string; r: number }>();
      for (const e of arr) {
        if (!e.exportName || !e.file) continue;
        // skip Trader-variant icons (ICO_Trader_*) — they're the trader-menu thumbnails, not the item.
        if (/^ico_trader_/i.test(e.exportName)) continue;
        const key = norm(e.exportName);
        if (!key) continue;
        const url = '/item-icons/' + String(e.file).replace(/^icons\//, '');
        const r = rank(e.file.toLowerCase());
        const cur = best.get(key);
        if (!cur || r < cur.r) best.set(key, { url, r });
      }
      const m = new Map<string, string>();
      for (const [k, v] of best) m.set(k, v.url);
      return m;
    })
    .catch(() => new Map<string, string>());
  return cache;
}

// Raw (code, url) pairs for building item pickers — keeps the original
// exportName as the display/insert value (loadIconMap normalizes it away).
// @dep: same /item-icons/_index.json as loadIconMap.
export interface IconEntry { code: string; url: string }
let entriesCache: Promise<IconEntry[]> | null = null;

export function loadIconEntries(): Promise<IconEntry[]> {
  if (entriesCache) return entriesCache;
  entriesCache = fetch('/item-icons/_index.json')
    .then((r) => r.json())
    .then((arr: Array<{ exportName?: string; file?: string }>) => {
      const seen = new Set<string>();
      const out: IconEntry[] = [];
      for (const e of arr) {
        if (!e.exportName || !e.file) continue;
        if (/^ico_trader_/i.test(e.exportName)) continue;
        const code = e.exportName.replace(/^ICO_/i, '');
        if (seen.has(code.toLowerCase())) continue;   // de-dupe variants, keep first
        seen.add(code.toLowerCase());
        out.push({ code, url: '/item-icons/' + String(e.file).replace(/^icons\//, '') });
      }
      return out.sort((a, b) => a.code.localeCompare(b.code));
    })
    .catch(() => []);
  return entriesCache;
}

// Resolve a code to an icon URL: exact -> loosened-exact -> best-substring fuzzy. null if nothing.
export function iconForCode(map: Map<string, string> | null, code: string): string | null {
  if (!map || !code) return null;
  const k = norm(code);
  if (!k) return null;
  const hit = map.get(k);
  if (hit) return hit;
  const lk = loosen(k);
  if (lk !== k && lk.length >= 3) {
    const lh = map.get(lk);
    if (lh) return lh;
  }
  // Fuzzy: score each icon key; prefer the longest specific overlap (avoids tiny false matches).
  let best: string | null = null;
  let bestScore = 0;
  for (const [key, url] of map) {
    if (key.length < 4) continue;
    let score = 0;
    if (k.includes(key) || lk.includes(key)) score = key.length;            // icon code is contained in the loot/tradeable code
    else if (key.includes(k)) score = k.length;                             // loot code is contained in a longer icon code
    else if (lk.length >= 4 && key.includes(lk)) score = lk.length;
    if (score > bestScore) { bestScore = score; best = url; }
  }
  return bestScore >= 4 ? best : null;
}
