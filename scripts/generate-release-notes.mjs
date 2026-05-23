#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const packageEntry = fileURLToPath(import.meta.resolve("conventional-changelog"));
const executable = join(dirname(packageEntry), "cli", "index.js");
const currentTag = currentReleaseTag();

const notes = execFileSync(
  process.execPath,
  [executable, "-p", "conventionalcommits", "-r", "2", "--stdout"],
  {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  },
);

process.stdout.write(
  extractReleaseSection(notes, currentTag) ?? fallbackReleaseSection(currentTag),
);

function extractReleaseSection(markdown, tag) {
  const headings = Array.from(markdown.matchAll(/^##\s+.+$/gm));
  const section = headings.find((heading) => {
    const headingTag = releaseTagFromHeading(heading[0]);
    return headingTag === tag || headingTag === tag.replace(/^v/, "");
  });

  if (!section) {
    return null;
  }

  const nextHeading = headings.find((heading) => heading.index > section.index);
  return markdown.slice(section.index, nextHeading?.index).trimStart();
}

function releaseTagFromHeading(heading) {
  const title = heading.replace(/^##\s+/, "").trim();
  const bracketed = title.match(/^\[([^\]]+)\]/);
  return bracketed?.[1] || title.split(/\s+/, 1)[0];
}

function currentReleaseTag() {
  const githubRef = process.env.GITHUB_REF_NAME?.trim();
  if (githubRef) {
    return githubRef;
  }

  try {
    return execFileSync("git", ["describe", "--tags", "--exact-match", "HEAD"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
  } catch {
    return execFileSync("git", ["describe", "--tags", "--abbrev=0", "HEAD"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
  }
}

function fallbackReleaseSection(tag) {
  const version = tag.replace(/^v/, "");
  const date = execFileSync("git", ["log", "-1", "--format=%cs", tag], {
    encoding: "utf8",
  }).trim();
  const previousTag = latestPreviousTag(tag);
  const range = previousTag ? `${previousTag}..${tag}` : tag;
  const commits = execFileSync(
    "git",
    ["log", "--reverse", "--pretty=format:%s", range],
    {
      encoding: "utf8",
    },
  )
    .split(/\r?\n/)
    .map((subject) => subject.trim())
    .filter(Boolean)
    .filter((subject) => !/^Bump version to \d+\.\d+\.\d+$/.test(subject));
  const body =
    commits.length > 0
      ? commits.map((subject) => `* ${subject}`).join("\n")
      : "* No user-facing changes recorded.";

  return `## ${version} (${date})\n\n${body}\n`;
}

function latestPreviousTag(currentTag) {
  const semverTagPattern = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
  const currentVersion = currentTag.match(semverTagPattern)?.[0]?.slice(1);
  if (!currentVersion) {
    return null;
  }

  const tags = execFileSync("git", ["tag", "--list", "v[0-9]*.[0-9]*.[0-9]*"], {
    encoding: "utf8",
  })
    .split(/\r?\n/)
    .map((tag) => tag.trim())
    .filter((tag) => tag && tag !== currentTag && semverTagPattern.test(tag));

  return (
    tags
      .filter((tag) => compareVersions(tag.slice(1), currentVersion) < 0)
      .sort((left, right) => compareVersions(right.slice(1), left.slice(1)))[0] ??
    null
  );
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
