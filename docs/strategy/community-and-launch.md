# TurdMOD Go‑to‑Market & Community Strategy

**Author:** Joel (Growth + Community Strategist)  
**Date:** 2026‑05‑23  
**Version:** 1.0 – For Internal Use Only  

> *Never giving any code up or secrets. Always protect us and our products. The motto is FOMO and being part of a community and open source.* – Joel

---

## 1. Brand Positioning

**Elevator Pitch**  
*TurdMOD is the only server‑side AI fleet and modding toolkit for SCUM that turns your server from a zombie‑infested chaos pit into a curated, living world – without giving a single fuck about your UE4 skills.*

**Taglines**

| Length | Text |
|--------|------|
| Long | “We cracked the UE4.27 server wide open so you can run a colony of AI bartenders, mechanics, and storytellers. Your players will never know they’re talking to a bot – but you’ll know because your player retention just doubled. Open‑source core, paid personas, and a FOMO‑driven marketplace that respects the hustle.” |
| Medium | “Ship your SCUM server into the future with an AI persona fleet. BSD‑licensed bridge, paid personas, limited beta seats. Join the turd revolution.” |
| Short | “TurdMOD. Because your server deserves friends that don’t scream at each other over loot.” |

**The Name**  
“Turdmod” sounds like a joke. That’s the point. When you own a name that makes people snicker, you’ve already won the first impression: *We’re not here to impress suits. We’re here to impress server admins who’ve been running on duct tape and prayer.* By the time they see the bridge primitives, the persona scaffold, and the CLI telemetry, they’ll know the name is just armor. The code is sharp.

---

## 2. Audience Segments (Acquisition Cost vs LTV)

| Segment | Est. Size | Acquisition Cost | LTV (12 mo) | Priority |
|---------|-----------|------------------|-------------|----------|
| **SCUM server admins (RP/PvE/PvP)** | ~3,000 active servers (est. 1 admins per) | Low ($2‑5) | $200‑600 | **Primary** – they pay for features that keep players. |
| Modding community (UE4 modders) | ~500 active / interested | Medium ($5‑15) | $0‑50 | **Secondary** – they contribute OSS code and drive credibility. |
| Content creators (YouTube/Twitch) | ~100 active SCUM streamers | High ($100‑500 sponsorship) | $0 (brand exposure) | **Tertiary** – leverage for demos, but don’t fund. |
| Players (downstream) | 50k+ players across active servers | Very Low ($0‑1) | $0 (only free) | **Secondary** – word‑of‑mouth, but don’t target advertising. |

**Key insight:** Admins are the paying customers. Serve them ruthlessly. Players are the reason admins pay. Keep players happy by making admins look like gods.

---

## 3. Channels – Content, Frequency, KPIs

### Reddit

| Subreddit | Content | Cadence | KPI |
|-----------|---------|---------|-----|
| r/SCUMgame (80k) | Demos, AMAs, persona reveals, server‑admin stories, “what if you could…?” polls | 2‑3x/week (Mon, Wed, Fri) | Upvotes (target 100+), comments, referral clicks |
| r/Unreal_4 / r/unrealengine (150k) | Technical deep dives: “How we RE’d UE4.27 server RPCs without binary cracks (the safe parts)”, bridge architecture | 1x/week | GitHub stars from modders |
| r/gamedev (1.7M) | “We built a server‑side AI fleet for a zombie game – here’s our networking stack” | 1x/month | Discussions, quality feedback (ignore upvotes) |

**Rules:** No RVAs, no crack code. Talk *what* we did, not *how* we circumvented security. Use screenshots of our own telemetry logger to prove we’re real.

### Discord

- **TurdMOD server:** Home base. Structure: `#welcome`, `#beta-signup`, `#bridge-oss`, `#persona-dev`, `#marketplace`, `#showcase`, `#admin-help`, `#changelog`, `#secret-channel` (paid Pro only).  
- **Presence in SCUM server community Discords:** Passive – answer questions, drop links when relevant. Do *not* spam.  
- **KPI:** DAU/MAU ratio > 0.4, message count, signups from invite links.

### YouTube

- **Channel:** `TurdMOD` (brand, not personal).  
- **Content calendar:**  
  - Demo videos (30‑90 seconds) – e.g., “Watch a Bouncer kick a PvP raider out of the safe zone in realtime.”  
  - Technical RE deep‑dives (5‑10 min) – “How we built a server‑side blueprint executor (safe parts)” – no RVAs.  
  - Admin tutorial playlist – “Deploy TurdMOD Lite on G‑Portal in 10 minutes.”  
- **Cadence:** 2 videos/week (1 demo, 1 tutorial or technical).  
- **KPIs:** Watch time > 50%, subscriber count, click‑through to landing page.

### Twitter / X

- **Account:** @turdmod  
- **Content:** Threads with technical snippets, GIFs of persona interactions, announcements, witty replies to SCUM dev tweets.  
- **Cadence:** 3‑5 tweets/day (automated with Buffer for 50%, real for 50%).  
- **KPIs:** Engagement rate > 3%, link clicks, follows.

### Twitch

- **Partnership:** Sponsor 1‑2 SCUM streamers who enjoy RP. Give them a private server with TurdMOD Pro for one stream. Demand they show the bots working. Pay $200‑500 per session.  
- **KPI:** Concurrent viewers during demo, chat mentions, VOD retention.

### GitHub

- **Repos:** [turdmod](https://github.com/roketteere/turdmod) and [scumpilot](https://github.com/roketteere/scumpilot).  
- **README quality:** Must be the best in the SCUM modding space. Include badges (build passing, license, Discord), a “Quick Start” that works in 5 commands, and a link to the persona showcase.  
- **Discussions tab:** Active – treat as a forum. Every bug report gets a thank‑you within 24h.  
- **KPI:** Stars, forks, issues opened, PRs merged.

---

## 4. FOMO + Scarcity Mechanics

### Founders’ Tier (First 100 Pro Subs)

- **Price locked at $15/mo for life** for everyone who subscribes before public launch. After launch, Pro goes to $30/mo.  
- **Badge in‑game:** “Founder” tag on the Bouncer persona when they greet players.  
- **Countdown clock** on landing page: “Only 83 seats left. Next price: $30/mo.”

### Beta Persona Exclusivity

- **Early access:** Paid beta testers get to request custom persona names (e.g., “Big Dick Johnny” for a Bouncer) that are permanently reserved.  
- **Beta‑only persona skins:** “Hologram” visual effect for the Doctor – never released to public.  
- **Limit:** Only 200 beta slots for the first wave. Then waitlist.

### Persona‑Name Reservations

- **Paid feature:** Reserve a custom display name for any persona (e.g., “Dr. Feelgood”) for $20 one‑time. Limited to one per persona type.  
- **Scarcity:** Only 50 reservations per persona category (e.g., only 50 “Dr. Feelgood” custom doctors exist across all servers).

### Marketplace Early‑Author Program

- **Creator sign‑ups:** Allow modders to publish their own persona scripts / blueprint packs on TurdMOD Marketplace. First 20 authors get 80% revenue share (instead of 60%).  
- **Exclusive marketplace items:** Only early authors can sell “Mastery Patches” (UI elements that unlock with custom code).  
- **Deadline:** Early‑author window closes 30 days after marketplace launch.

---

## 5. Open‑Source Narrative

### What’s MIT‑Licensed

- **Bridge framework** (cpp) – the UE4.27 RPC layer that connects server to external software.  
- **Persona scaffold** (TypeScript) – the boilerplate for creating a new persona.  
- **CLI tools** – `turdctl` for managing personas from terminal.  
- **Recipes** – JSON files that map persona behaviors to server events (open source so the community can hack them).

### What’s Commercial

- **Premium personas** – the 10 handcrafted personas (Bouncer, Doctor, etc.). Each comes with unique animations, voice lines (if we ever crack that), and curated behavior trees.  
- **Marketplace** – the *platform* itself is free, but every transaction takes a 20% cut.  
- **Hosted Multi‑Tenant** – a paid SaaS tier where we run the bridge on our servers for lazy admins (future).

### The Story

> *“We built the engine in the open so you can trust it. Pay for our personas, persona‑curated content, and hosted convenience. You can run our open‑source bridge and write your own personas for free – but that’s like building your own oven to bake a frozen pizza. Both work, but one tastes a lot better and comes with a warranty.”*

This narrative builds credibility (code is verifiable) without giving away the secret sauce. Admins love transparency; they also value time. We sell time.

---

## 6. Content Calendar – First 90 Days

| Week | Theme | Content | Channel | FOMO Trigger |
|------|-------|---------|---------|--------------|
| 1‑2 | **Launch Teaser** | “I built an AI bot that runs my SCUM server while I sleep. AMA.” (Reddit) + “5‑min demo” (YouTube) + GitHub README rewrite + Discord welcome post | Reddit, YouTube, GitHub, Discord | Open beta waitlist signup |
| 3‑4 | **Persona Reveals** | One per week: Bouncer, Doctor, Mechanic, Quartermaster. Each gets a dramatic video (e.g., “Meet the Bouncer – your new door policy”). | YouTube, Twitter, Reddit | Pre‑order Pro with founder’s price (only 100 seats left) |
| 5‑8 | **Adoption Phase** | Server admin testimonials (text + video). “How TurdMOD tripled my player retention.” + case study blog posts. | Reddit, YouTube, Discord | Beta slots drop to 50 remaining |
| 9‑12 | **Marketplace Launch** | Open marketplace to early authors. “Build your own persona and sell it.” + creator program announcement. | Discord, GitHub, Reddit, Twitter | Early‑author revenue share window closing (only 10 days left) |

**Additional evergreen content:** Technical RE deep‑dives every two weeks (targeting r/unrealengine). Provide *just enough* tech to impress modders, never enough to compromise security.

---

## 7. Anti‑Pattern Catalog – What NOT to Do

| Anti‑Pattern | Why It’s Bad | How to Avoid |
|--------------|--------------|--------------|
| Reveal Layer 3 crack details | Immediate patching by G‑Portal/SCUM devs, potential takedown of the whole project | Keep all crack internals in `.secrets/` and never mention them in public. Redirect technical questions to “we use a custom UE4.27 bridge layer that we cannot discuss for security reasons.” |
| Post RVAs / sigscan output | Same as above – gives cheat devs a free lunch, gets us banned from hosting providers | Redact all memory addresses. Use abstract diagrams (e.g., “this function hooks the network exec”). |
| Promise features before shipping | Kills trust. Example: “We’ll have custom vehicles by July” – delays ruin momentum | Only talk about **shipped** features. Under‑promise, over‑deliver. |
| Fight with SCUM devs | They control the game. A public spat can cause them to kill modding via EULA changes | Where possible, reach out to them privately. Offer to be a “partner” in improving server tools. Never badmouth them publicly. |
| Get bogged down in player feature debates | Players ask for things that make admins’ lives harder (e.g., “give everyone admin”). You serve admins. | In community channels, keep the focus on admin experience. Have a pinned FAQ: “We build for admins. Players are our users’ users.” |

---

## 8. Operational Playbook

| Role | Person | Tools |
|------|--------|-------|
| **Launch Coordinator** | Joel (you) | Notion (content calendar), Buffer (scheduling), GitHub (releases) |
| **Discord Mod Team** | 3‑5 volunteer power users (recruit from early beta) | Discord built‑in mod tools, a shared Google Sheet for bans/approvals |
| **Content Creator** | Joel + AI‑generated supplement (use ChatGPT for script outlines, manual polish) | DaVinci Resolve (editing), OBS (capture), Midjourney (thumbnail art) |
| **Community Manager** | Joel + AI‑generated responses (use a custom GPT trained on FAQs) | Buffer/X Pro, Reddit scheduler (TBD), Discord bot for automated replies |
| **Tools** | Buffer/Hootsuite for scheduling, Discord for real‑time, Notion for calendar | |

**Cadence:** Joel spends 2h/day on community + content for the first 3 months. After that, hire a CM part‑time if growth justifies.

---

## 9. The First 5 Posts (Actual Draft Text)

### Post #1 – Reddit r/SCUMgame (Week 1)

**Title:** I built an AI bot that runs my SCUM server while I sleep. AMA.  

**Body:**  

> Yo,
>
> I’m the guy who runs [Server Name], a hard‑core RP server with 30 active players. Two weeks ago I was manually kicking griefers and roleplaying as a bartender every night. Now I have an AI persona fleet that does all that for me. Bouncer checks gear at the gate, Doctor heals players (with a 3% chance of giving them a fake diagnosis), and Mechanic sells vehicle repairs – all stored in a server‑side wallet.
>
> This is TurdMOD. We reverse‑engineered SCUM’s server networking (the legal parts) to let you run AI scripts that hook into player interactions. The core bridge is open source. The personas are paid (but cheap).
>
> **Proof:** Here’s a video of the Bouncer telling a loot goblin to “fuck off” and then banning them from the safe zone for 10 minutes. (link)
>
> **The point:** You don’t need to be a modder. You don’t need to know UE4. You just need a server and $15/month. First 100 founders get locked‑in pricing.
>
> AMA – I’ll answer everything except the stuff that gets us patched.
>
> – Joel

### Post #2 – YouTube (Week 1)

**Title:** TurdMOD in 5 Minutes – From Chaos to Managed Server in One Evening  

**Description:**  
> No bullshit. You’ll see me install TurdMOD Lite on a G‑Portal server via FTP, start the bridge, spawn a Bouncer persona, and watch it handle player interactions in real‑time. Then I upgrade to Pro and show the Storyteller persona creating dynamic quests.  
>  
> **Links:**  
> GitHub (free bridge): [link]  
> Discord (beta signup): [link]  
>  
> No RVAs, no crack code, just a server admin’s dream.

### Post #3 – Twitter/X Thread (Week 1)

**Tweet 1:**  
> We reverse‑engineered SCUM’s server RPCs to build a modding bridge. No, we won’t tell you how. Yes, it works on every hosting provider.  
>  
**Tweet 2:**  
> The bridge is open source (MIT). Go look at it. The bridge is clean. The personas are the secret sauce.  
>  
**Tweet 3:**  
> First 100 Pro subscribers get to choose their persona’s custom display name. Only one “Dr. Fuck” exists. Rush or miss out.  
>  
**Tweet 4:**  
> We also built a telemetry dashboard that shows your server’s FPS, memory, and RTT – all from the bridge. No third‑party tools.  
>  
**Tweet 5:**  
> Beta starts next week. Waitlist open now. Limited to 200 seats.  
>  
> Link: [signup page]

### Post #4 – Discord Pinned Welcome (after server creation)

```
Welcome to the TurdMOD server.

We are the only place you can get:
- Free bridge that works on ANY SCUM hosting
- AI personas that actually roleplay
- A marketplace where modders can sell their own stuff

Rules:
1. No asking for crack code. We will ban you.
2. Be helpful or be silent.
3. Admins > Players. This is an admin tool.

We are in beta. Be patient – we’re still building the plane while flying.

Type !whitelist to join the beta queue.
```

### Post #5 – GitHub README Rewrite (Week 1)

```markdown
# TurdMOD – The SCUM Server Sidecar

**Because your server deserves friends who don't complain about loot tables.**

TurdMOD is a server‑side modding framework for SCUM (UE4.27). It runs as a separate process, connects to your game server via our custom RPC bridge, and gives you full control over player interactions, AI behaviors, and server automation.

## Features

- **Bridge primitives (56 RPC handlers)** – chat, inventory, spawn, damage, teleport, and more. All open source.
- **Persona scaffold** – write your own AI bots in TypeScript. Tutorials included.
- **CLI tool** – `turdctl` for deploying and monitoring from terminal.
- **Server telemetry** – real‑time performance data without external tools.
- **Premium personas** – Bouncer, Doctor, Mechanic, Quartermaster, Trader, Architect, Hunter, Storyteller, Counselor, Conductor. $15‑100/month.

## Quick Start (Lite – Free)

1. Download the latest release.
2. Upload `bridge.dll` and `config.json` to your server’s `[GameDir]/Binaries/Win64/`.
3. Run `turdctl start`.
4. Connect to your server and type /turd welcome.

See the [docs](docs/) for detailed setup on G‑Portal, Nitrado, or custom VPS.

## Quick Start (Pro)

1. Subscribe at [turdmod.com](https://turdmod.com) (founder’s price: $15/mo – first 100 only).
2. Follow the Pro installation guide for UE4SS bridge.
3. Activate your persona fleet via the remote GUI.

## Contributing

The bridge is MIT. Fork it, improve it, send PRs. Premium personas are proprietary – we sell them to fund this whole circus.

## License

Bridge, CLI, scaffold, and recipes: MIT. Premium personas: proprietary. Security internals (`.secrets/`): never published.
```

---

## 10. Metrics + Checkpoints

| Month | Discord Members | Paid Beta Users | GitHub Stars | LTV Target | CAC Budget |
|-------|-----------------|-----------------|--------------|------------|------------|
| 1     | 500             | 10              | 50           | $50/yr avg | $5/free, $50/paid |
| 3     | 5,000           | 100             | 500          | $50/yr avg | $5/free, $50/paid |
| 6     | 20,000          | 500             | 2,000        | $50/yr avg | $5/free, $50/paid |

**Checkpoint actions:**
- **Month 1:** If paid users <10, double down on Reddit AMAs and offer early‑author marketplace slots for free personas.
- **Month 3:** If Discord <5k, launch a referral program (“Invite an admin, get 1 month free Pro”).
- **Month 6:** If LTV <$40, reconsider pricing or add a higher‑tier with custom blueprint pak content (once Layer 3 is ready).

**Attribution:** Track signups via UTM codes per channel. Use a simple dashboard (Google Analytics + Discord bot).

---

*This document is a living strategy. Review monthly, update quarterly, and never forget: we sell the power to turn a game server into a goddamn civilization. Keep the FOMO high, the code open where it matters, and the secrets under lock.*