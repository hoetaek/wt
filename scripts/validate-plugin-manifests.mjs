#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const errors = [];
const fail = (message) => errors.push(message);

const readJSON = (rel) => {
  const path = join(ROOT, rel);
  if (!existsSync(path)) {
    fail(`missing file: ${rel}`);
    return null;
  }
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail(`invalid JSON in ${rel}: ${error.message}`);
    return null;
  }
};

const cargoToml = readFileSync(join(ROOT, "Cargo.toml"), "utf8");
const cargoVersion = cargoToml.match(/^version = "([^"]+)"/m)?.[1];
if (!cargoVersion) fail("Cargo.toml: missing package version");

const ccPlugin = readJSON("plugins/wt/.claude-plugin/plugin.json");
const codexPlugin = readJSON("plugins/wt/.codex-plugin/plugin.json");
const ccMarket = readJSON(".claude-plugin/marketplace.json");
const codexMarket = readJSON(".agents/plugins/marketplace.json");

if (ccPlugin?.name !== "wt") fail("Claude plugin name must be wt");
if (codexPlugin?.name !== "wt") fail("Codex plugin name must be wt");
if (codexPlugin?.skills !== "./skills/") fail("Codex plugin skills must be ./skills/");
if (!codexPlugin?.interface?.displayName) fail("Codex plugin missing interface.displayName");

const ccEntry = ccMarket?.plugins?.find((plugin) => plugin.name === "wt");
const codexEntry = codexMarket?.plugins?.find((plugin) => plugin.name === "wt");
if (!ccEntry) fail("Claude marketplace missing wt entry");
if (!codexEntry) fail("Codex marketplace missing wt entry");
if (ccEntry?.source !== "./plugins/wt") fail("Claude marketplace source must be ./plugins/wt");
if (codexEntry?.source?.path !== "./plugins/wt") fail("Codex marketplace source.path must be ./plugins/wt");

const versions = {
  "Cargo.toml": cargoVersion,
  "Claude plugin": ccPlugin?.version,
  "Codex plugin": codexPlugin?.version,
  "Claude marketplace metadata": ccMarket?.metadata?.version,
  "Claude marketplace entry": ccEntry?.version,
  "Codex marketplace metadata": codexMarket?.metadata?.version,
  "Codex marketplace entry": codexEntry?.version,
};
for (const [name, version] of Object.entries(versions)) {
  if (!version) fail(`${name}: missing version`);
  if (cargoVersion && version && version !== cargoVersion) {
    fail(`${name}: version ${version} does not match Cargo.toml ${cargoVersion}`);
  }
}

const skillsDir = join(ROOT, "plugins/wt/skills");
if (!existsSync(skillsDir)) {
  fail("missing plugins/wt/skills/");
} else {
  const skills = readdirSync(skillsDir).filter((name) => {
    const path = join(skillsDir, name);
    return statSync(path).isDirectory() && existsSync(join(path, "SKILL.md"));
  });
  if (skills.length === 0) fail("plugins/wt/skills/ has no skill with SKILL.md");
  console.log(`ok skills: ${skills.sort().join(", ")}`);
}

if (existsSync(join(ROOT, "skills"))) fail("root skills/ should not exist; use plugins/wt/skills/");

if (errors.length) {
  console.error("plugin manifest validation failed:");
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

console.log(`ok manifests: wt ${cargoVersion}`);
