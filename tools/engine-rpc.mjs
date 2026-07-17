#!/usr/bin/env node
// engine-rpc — drive the TurdMOD engine bridge from the shell, using the
// SAME config the Manager uses. First-class access to every bridge RPC
// without the desktop app or button clicks.
//
//   node tools/engine-rpc.mjs <target> <method> [paramsJson]
//
//   target  = remote | local   (default: remote = OVH)
//   method  = any bridge RPC method name
//   params  = JSON object (default {})
//
// Examples:
//   node tools/engine-rpc.mjs remote getOnlinePlayers
//   node tools/engine-rpc.mjs remote dumpAdminCommands
//   node tools/engine-rpc.mjs remote runAdminCommand '{"command":"SetWeather 0","playerName":"Lilac","gameThread":"1","bypass":"1"}'
//   node tools/engine-rpc.mjs remote getAdminOutput

import { callEngine } from './lib/engine-client.mjs';

async function main() {
  const [, , targetArg, method, paramsArg] = process.argv;
  if (!method) {
    console.error('usage: engine-rpc <remote|local> <method> [paramsJson]');
    process.exit(2);
  }
  let params = {};
  if (paramsArg) {
    try { params = JSON.parse(paramsArg); }
    catch { console.error('paramsJson is not valid JSON'); process.exit(2); }
  }
  try {
    const result = await callEngine(targetArg, method, params);
    console.log(JSON.stringify(result, null, 2));
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
}

main();
