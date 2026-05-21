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

process.stdout.write(extractReleaseSection(notes, currentTag));

function extractReleaseSection(markdown, tag) {
  const headings = Array.from(markdown.matchAll(/^##\s+.+$/gm));
  const section = headings.find((heading) => {
    const headingTag = releaseTagFromHeading(heading[0]);
    return headingTag === tag || headingTag === tag.replace(/^v/, "");
  });

  if (!section) {
    throw new Error(`Release notes for ${tag} were not generated.`);
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
