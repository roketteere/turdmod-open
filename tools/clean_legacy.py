P = r"C:\Development\Claude\turdmod\apps\turdmod-service\src\legacy.rs"
NAMES = ["chat_cmds", "permissions", "teleport", "npc_services", "pvp_rotation", "vehicle_repo"]
lines = open(P, encoding="utf-8").read().splitlines(keepends=True)
out, removed = [], []
for ln in lines:
    hit = next((n for n in NAMES if f'("{n}")' in ln), None)
    if hit:
        removed.append(hit)
    else:
        out.append(ln)
open(P, "w", encoding="utf-8", newline="\n").write("".join(out))
print("REMOVED:", sorted(set(removed)), "count=", len(removed))
print("NOT_FOUND:", [n for n in NAMES if n not in removed])
