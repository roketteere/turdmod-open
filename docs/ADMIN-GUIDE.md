# TurdMOD Server — Complete Admin & Player Reference

**83 modules | 99 bridge handlers | 130+ commands | 4 permission tiers**

## Permission Tiers

| Tier | Level | Access |
|---|---|---|
| Free | 0 | Economy, teleport, leaderboard, voting, quests, achievements, reputation, duels, trading, companions |
| Premium | 1 | Vehicle ownership, kits, banking, auction, player shops, fast travel, map markers, convoy, fishing |
| VIP | 2 | Gambling, lottery, supply drops, skill boost |
| Admin | 3 | God mode, metabolism, jail, warzone, horde, boss fight, airdrop, safe zones, mechman |

Admin commands: `!perm grant <player> <tier>` | `!perm mod <mod> on/off` | `!perm set <player> <mod> on/off`

## Quick Reference — All Commands

### Economy (Free)
```
!balance          — Check coin balance
!daily            — Claim 100-coin daily bonus
!pay Player 50    — Send 50 coins to Player
!top              — Top 5 richest players
!bounty Player 200 — Place bounty on Player
```

### Banking (Premium)
```
!bank             — Wallet + bank balance (1%/hr interest)
!deposit 500      — Move to bank
!withdraw 200     — Take from bank
```

### Vehicle Ownership (Premium)
```
!register         — Sit in vehicle first! Registers to your Steam ID
!myride           — Show your registered vehicles
!transfer Player  — Initiate ownership transfer
!yes / !no        — Accept or deny transfer
!unregister       — Release ownership
```

### Combat (Free)
```
!kd               — Your kill/death stats
!leaderboard      — Top 5 killers
!duel Player      — Challenge to 1v1
!accept / !decline — Respond to duel challenge
```

### Social (Free)
```
!clan create Name — Found a clan
!clan invite Player — Invite (leader only)
!clan accept      — Join invited clan
!clan war Clan    — Declare war
!clan peace Clan  — End war
!rep              — Check reputation
!thank Player     — Give +3 rep
```

### Admin Commands
```
!day / !night     — Set time
!weather 0.5      — Set weather (0=clear, 1=storm)
!storm / !clear   — Quick weather
!fly / !fly off   — Toggle flying
!god              — Toggle god mode
!heal Player      — Heal
!feed Player      — Give food
!jail Player 10   — Jail for 10 min
!warzone          — Start PvP event
!horde 5          — 5 zombie waves
!boss             — 5-wave boss fight
!airdrop Player   — Supply drop
!perm grant Player premium — Set tier
```

For full documentation with examples, see the complete reference below.
