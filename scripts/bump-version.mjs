#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2] ?? "";
const semverPattern = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

if (!semverPattern.test(version)) {
  fail(`Usage: pnpm run bump X.Y.Z`);
}

updateJsonVersion("package.json", version);
updateJsonVersion("src-tauri/tauri.conf.json", version);
updateCargoVersion("Cargo.toml", version);
updateCargoVersion("src-tauri/Cargo.toml", version);

execFileSync("cargo", ["metadata", "--format-version=1"], {
  stdio: "ignore",
});
execFileSync("node", ["scripts/validate-release-version.mjs", `v${version}`], {
  stdio: "inherit",
});

console.log(`Updated Wovo version to ${version}.`);

function updateJsonVersion(path, nextVersion) {
  const json = JSON.parse(readFileSync(path, "utf8"));
  json.version = nextVersion;
  writeFileSync(path, `${JSON.stringify(json, null, 2)}\n`);
}

function updateCargoVersion(path, nextVersion) {
  const contents = readFileSync(path, "utf8");
  const next = contents.replace(
    /^version\s*=\s*"[^"]+"/m,
    `version = "${nextVersion}"`,
  );
  if (contents === next) {
    fail(`Could not find package version in ${path}.`);
  }
  writeFileSync(path, next);
}

function fail(message) {
  console.error(message);
  process.exit(1);
}
