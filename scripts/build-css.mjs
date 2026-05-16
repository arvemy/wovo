#!/usr/bin/env node
import { spawn } from "node:child_process";

const isWindows = process.platform === "win32";
const command = isWindows ? "pnpm.cmd" : "pnpm";
const child = spawn(command, ["run", "build:css"], {
  env: process.env,
  shell: isWindows,
  stdio: "inherit",
});

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
