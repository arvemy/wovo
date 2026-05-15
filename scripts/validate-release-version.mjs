#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const releaseTag = process.argv[2] ?? process.env.GITHUB_REF_NAME ?? "";
const semverTagPattern = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const match = releaseTag.match(semverTagPattern);

if (!match) {
  fail(`Release tag must use vMAJOR.MINOR.PATCH SemVer format. Got: ${releaseTag || "<empty>"}`);
}

const version = releaseTag.slice(1);
const expectedVersions = [
  ["src-tauri/tauri.conf.json", JSON.parse(read("src-tauri/tauri.conf.json")).version],
  ["package.json", JSON.parse(read("package.json")).version],
  ["src-tauri/Cargo.toml", cargoVersion("src-tauri/Cargo.toml")],
  ["Cargo.toml", cargoVersion("Cargo.toml")],
];

const mismatches = expectedVersions.filter(([, actual]) => actual !== version);
if (mismatches.length > 0) {
  fail(
    [
      `Release tag ${releaseTag} does not match all manifest versions (${version}).`,
      ...mismatches.map(([file, actual]) => `- ${file}: ${actual}`),
    ].join("\n"),
  );
}

const previousTag = latestPreviousTag(releaseTag);
if (previousTag && compareVersions(version, previousTag.slice(1)) <= 0) {
  fail(`Release tag ${releaseTag} must be greater than previous release tag ${previousTag}.`);
}

console.log(`Validated release version ${version}.`);

function read(path) {
  return readFileSync(path, "utf8");
}

function cargoVersion(path) {
  const match = read(path).match(/^\s*version\s*=\s*"([^"]+)"/m);
  if (!match) {
    fail(`Could not find package version in ${path}.`);
  }
  return match[1];
}

function latestPreviousTag(currentTag) {
  let tags;
  try {
    tags = execFileSync("git", ["tag", "--list", "v[0-9]*.[0-9]*.[0-9]*"], {
      encoding: "utf8",
    })
      .split(/\r?\n/)
      .map((tag) => tag.trim())
      .filter((tag) => tag && tag !== currentTag && semverTagPattern.test(tag));
  } catch {
    return null;
  }

  return tags.sort((a, b) => compareVersions(b.slice(1), a.slice(1)))[0] ?? null;
}

function compareVersions(left, right) {
  const leftParts = left.split(".").map(Number);
  const rightParts = right.split(".").map(Number);

  for (let index = 0; index < 3; index += 1) {
    if (leftParts[index] !== rightParts[index]) {
      return leftParts[index] - rightParts[index];
    }
  }

  return 0;
}

function fail(message) {
  console.error(message);
  process.exit(1);
}
