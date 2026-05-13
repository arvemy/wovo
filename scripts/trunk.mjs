#!/usr/bin/env node
import { spawn } from "node:child_process";

const args = process.argv.slice(2);

if (args.length === 0) {
  console.error("Usage: node scripts/trunk.mjs <trunk-arguments...>");
  process.exit(2);
}

// Trunk's color parser rejects common NO_COLOR values such as "1".
const env = { ...process.env, NO_COLOR: "true" };
const command = process.platform === "win32" ? "trunk.cmd" : "trunk";
const child = spawn(command, args, { env, stdio: "inherit" });

child.on("error", (error) => {
  console.error(`Failed to start ${command}: ${error.message}`);
  process.exit(1);
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    child.kill(signal);
  });
}

child.on("exit", (code, signal) => {
  if (signal === "SIGINT") {
    process.exit(130);
  }
  if (signal === "SIGTERM") {
    process.exit(143);
  }
  process.exit(code ?? 1);
});
