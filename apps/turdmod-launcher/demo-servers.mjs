// Tiny standalone allowlist server for LOCAL DEMO only.
// Serves the exact GET /api/servers shape the launcher expects, with the OVH
// production entry injected, so OVH shows up in the launcher's picker WITHOUT
// running the full turdmod-web app or touching the public turdmod.com site.
// @inv: local-demo only — do NOT use to expose OVH publicly (legal HOLD).
import { createServer } from "node:http";

const SERVERS = [
  {
    id: "ovh-prod",
    name: "TurdMOD OVH Production",
    ip: "YOUR_SERVER_IP",
    port: 7042,
    battlEye: false,
    region: "NA-East",
    description: "Engine-tier production server. BattlEye off. Modded client only.",
  },
];

const PORT = Number(process.env.PORT) || 3000;

createServer((req, res) => {
  res.setHeader("Access-Control-Allow-Origin", "*");
  const url = new URL(req.url, `http://localhost:${PORT}`);
  if (url.pathname === "/api/servers") {
    const all = url.searchParams.get("all") === "1";
    const list = all ? SERVERS : SERVERS.filter((s) => s.battlEye === false);
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ servers: list, total: list.length }));
    return;
  }
  res.writeHead(404, { "Content-Type": "application/json" });
  res.end(JSON.stringify({ error: "not found" }));
}).listen(PORT, () => {
  console.log(`[demo-servers] GET http://localhost:${PORT}/api/servers — OVH allowlist live`);
});
