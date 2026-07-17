# 7-to-21 Point Inspection — TurdMOD QA Protocol

How we verify every deploy before it hits production. The 7-point is the fast gate; the 21-point is the full sweep. Adapted from the ScummyMap QA protocol.

---

## 7-Point Inspection (Fast Gate)

Run after every commit, before every deploy. Takes ~30 seconds.

| # | Check | Command | What it catches |
|---|-------|---------|-----------------|
| 1 | **TypeScript typecheck** | `npx tsc --noEmit` (from apps/turdmod-web/) | Type errors, broken imports, missing props |
| 2 | **Zero turd-* tokens** | `grep -r "turd-" src/ --include="*.tsx"` | Old palette leaking into themed pages |
| 3 | **All routes 200** | `curl` each public route | 404s, 500s, missing pages |
| 4 | **Static assets load** | Check `/_next/static/` URLs from page source | CSS/JS 404s from bad deploy |
| 5 | **No hardcoded localhost** | `grep -r "localhost" src/ --include="*.tsx"` | Dev URLs leaking to production |
| 6 | **Bridge DLL compiles** | `tmp\build-bridge.cmd` | C++ errors in engine handlers |
| 7 | **Manager typecheck** | `npx tsc --noEmit` (from apps/turdmod-manager/) | Manager UI type errors |

**Pass criteria:** All 7 green. Any red = stop and fix before deploying.

---

## 21-Point Inspection (Full Sweep)

Run before major releases, after large refactors, or when touching shared infrastructure (layout, auth, bridge, companion runtime).

### Website Routes & Links (1–4)

| # | Check | What it validates |
|---|-------|-------------------|
| 1 | **Nav href → route resolution** | Every `href` in NAV_LINKS (layout.tsx) has a matching `page.tsx` in `src/app/`. No dead links. |
| 2 | **Component import chain** | Every `from '@/...'` and `from './...'` import resolves to a real file. No broken imports. |
| 3 | **Tailwind token coverage** | Every x12-* class used in source exists in tailwind.config.ts. No undefined tokens. |
| 4 | **Hover state coverage** | No `hover:bg-X` where X equals the resting state (no-op hovers). All buttons have visible hover feedback. |

### SEO & Metadata (5–8)

| # | Check | What it validates |
|---|-------|-------------------|
| 5 | **Metadata completeness** | Every page.tsx has `export const metadata` with `title` and `description`. |
| 6 | **robots.ts rules** | Allows `/`, disallows `/api/{admin,auth,desktop,stripe}`. Has sitemap reference. |
| 7 | **sitemap.ts structure** | Covers static pages + published mods. Degrades gracefully if DB is down. |
| 8 | **OG image** | `opengraph-image.tsx` exists, uses ImageResponse, correct dimensions. |

### Design Consistency (9–12)

| # | Check | What it validates |
|---|-------|-------------------|
| 9 | **x12-* palette only** | Zero `turd-*` tokens in any .tsx source file (Manager uses turd-*, web uses x12-*). |
| 10 | **Responsive layout** | Root layout uses `max-w-7xl px-4 lg:px-8`. All pages render at 375px, 768px, 1440px. |
| 11 | **Mobile nav works** | MobileNav component imported, renders hamburger on mobile, drawer slides in. |
| 12 | **Footer on all pages** | Footer renders in layout.tsx outside `{children}`. ScummyMap branding present. |

### Bridge & Engine (13–16)

| # | Check | What it validates |
|---|-------|-------------------|
| 13 | **Bridge compiles** | `tmp\build-bridge.cmd` exits 0. DLL at expected path. |
| 14 | **Handler count** | `grep "&handle_" dllmain.cpp \| wc -l` matches expected count (75+). |
| 15 | **New handlers registered** | Any new `handle_xxx` function has a corresponding `{ "xxx", &handle_xxx }` entry in the dispatch table. |
| 16 | **Forward declarations** | Any function called before its definition has a forward declaration near line 955. |

### Companion & Mods (17–19)

| # | Check | What it validates |
|---|-------|-------------------|
| 17 | **All mods have manifests** | Every dir in `mods/` has `turdmod.json` with id, name, version, entrypoint. |
| 18 | **All mods have sidecars** | Every dir in `mods/` has `.turdmod-installed.json` for Library detection. |
| 19 | **Companion loads all mods** | Start companion with TURDMOD_MODS_DIR → all mods log "loaded" without errors. |

### Release Readiness (20–21)

| # | Check | What it validates |
|---|-------|-------------------|
| 20 | **No forgotten files** | `git status` shows no untracked source files. Everything committed. |
| 21 | **Full build validation** | `pnpm build` in apps/turdmod-web/ exits 0. `item-catalog.json` parses as valid JSON. |

---

## How to Run

```bash
# 7-point (fast gate)
cd apps/turdmod-web && npx tsc --noEmit
grep -r "turd-" src/ --include="*.tsx" | wc -l  # should be 0
# curl each route (see QA agent pattern)
cd apps/turdmod-manager && npx tsc --noEmit

# 21-point (full sweep) — manual for now, automate with scripts/inspect-21.mjs later
```

---

## When to Use Which

| Situation | Gate |
|-----------|------|
| Normal commit + push | 7-point |
| New page or route added | 21-point |
| Bridge handler added/modified | 21-point |
| Layout, nav, or auth changed | 21-point |
| Companion mod added/modified | 21-point |
| Pre-deploy to turdmod.com | 21-point |
| Hotfix (single-file, isolated) | 7-point |
| Website redesign / palette change | 21-point |

---

*Protocol established 2026-05-24, adapted from ScummyMap's 7To21Inspection.md.*
*Maintained by Joel Perez and Claude.*
