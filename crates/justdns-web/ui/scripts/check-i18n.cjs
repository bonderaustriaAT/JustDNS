// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

/* Keeps the translation catalogues honest:
 *  - DE and EN must carry exactly the same keys (the Catalog type enforces this
 *    at compile time for EN; this also catches duplicates and stray keys).
 *  - Placeholders ({name}) must match between the languages — a missing or
 *    extra placeholder only shows up as a broken sentence at runtime.
 *  - Every t("...") call must name a key that exists.
 *  - Keys no source file references (directly, via a typed labelKey-style prop,
 *    or through a template literal's constant prefix) are listed as candidates
 *    for deletion. Informational: keys built from fully dynamic expressions are
 *    invisible to every static check, so this never fails the build.
 *
 * Usage: npm run check:i18n */

"use strict";

const fs = require("fs");
const path = require("path");

const root = path.resolve(__dirname, "..");
const src = fs.readFileSync(path.join(root, "src/lib/messages.ts"), "utf8");

function fail(message) {
  console.error("check:i18n: " + message);
  process.exitCode = 1;
}

// ---- Catalogue extraction -------------------------------------------------

const keyRegex = /^ {2}"([a-zA-Z0-9_.]+)":/gm;
const keys = (s) => [...s.matchAll(keyRegex)].map((m) => m[1]);
const enIdx = src.search(/const en: Catalog = \{/);
if (enIdx < 0) fail("could not find the `en` catalogue — checker needs updating");
const deBlock = src.slice(0, enIdx);
const enBlock = src.slice(enIdx);
const de = keys(deBlock);
const en = keys(enBlock);
const deSet = new Set(de);
const enSet = new Set(en);

if (de.length === 0) fail("no German keys found — checker needs updating");

const missingInEn = de.filter((k) => !enSet.has(k));
const missingInDe = en.filter((k) => !deSet.has(k));
const dup = (a) => a.filter((k, i) => a.indexOf(k) !== i);
if (missingInEn.length) fail("keys missing from EN: " + JSON.stringify(missingInEn));
if (missingInDe.length) fail("keys missing from DE: " + JSON.stringify(missingInDe));
if (dup(de).length) fail("duplicate keys in DE: " + JSON.stringify(dup(de)));
if (dup(en).length) fail("duplicate keys in EN: " + JSON.stringify(dup(en)));

// Placeholders must agree per key across the languages.
const placeholders = (block) => {
  const map = {};
  for (const m of block.matchAll(/^ {2}"([a-zA-Z0-9_.]+)": *"([^"]*)"/gm)) {
    map[m[1]] = [...m[2].matchAll(/\{([a-z]+)\}/g)].map((x) => x[1]).sort().join(",");
  }
  return map;
};
const dePh = placeholders(deBlock);
const enPh = placeholders(enBlock);
for (const key of Object.keys(dePh)) {
  if (dePh[key] !== enPh[key]) {
    fail("placeholder mismatch on '" + key + "': DE(" + dePh[key] + ") EN(" + enPh[key] + ")");
  }
}

// ---- Reference extraction --------------------------------------------------

function walk(dir) {
  let out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out = out.concat(walk(full));
    else if (/\.(tsx|ts)$/.test(entry.name)) out.push(full);
  }
  return out;
}
const files = walk(path.join(root, "src")).filter(
  (f) => !f.endsWith("messages.ts") && !f.endsWith("i18n.tsx"),
);

const used = new Set();
for (const file of files) {
  const code = fs.readFileSync(file, "utf8");
  // Every dotted string literal, so typed references (labelKey: "nav.tab.x",
  // phase maps, CLUSTER_PHASE_KEY tables) count as usage too.
  for (const m of code.matchAll(/"([a-z][a-zA-Z0-9]*(?:\.[a-zA-Z0-9_]+)+)"/g)) {
    used.add(m[1]);
  }
  for (const m of code.matchAll(/'([a-z][a-zA-Z0-9]*(?:\.[a-zA-Z0-9_]+)+)'/g)) {
    used.add(m[1]);
  }
  // Constant prefixes of template-literal keys: `phishing.feed.${key}.name`
  // marks every `phishing.feed.*` key as reachable. The prefix may end on a
  // separating dot (`update.phase.${phase}` reaches update.phase.recreating).
  for (const m of code.matchAll(/`([a-z][a-zA-Z0-9]*(?:\.[a-zA-Z0-9_]+)*\.?)\$\{/g)) {
    for (const key of deSet) {
      if (key.startsWith(m[1])) used.add(key);
    }
  }
}

// A t("...") call naming something outside the catalogue is always a bug.
for (const file of files) {
  const code = fs.readFileSync(file, "utf8");
  for (const m of code.matchAll(/(?<![a-zA-Z0-9_.])t\("([a-zA-Z0-9_.]+)"/g)) {
    if (!deSet.has(m[1])) fail(file + " calls t(\"" + m[1] + "\") which is not in the catalogue");
  }
}

// ---- Report ----------------------------------------------------------------

const dead = de.filter((k) => !used.has(k));
console.log(
  "check:i18n: " + de.length + " keys, DE/EN parity OK, placeholders OK, t() calls OK",
);
if (dead.length) {
  console.log("check:i18n: " + dead.length + " unreferenced key(s) — candidates for deletion:");
  for (const key of dead) console.log("  - " + key);
  console.log(
    "check:i18n: informational only — keys built from fully dynamic expressions" +
      " are invisible to a static check, so review before deleting.",
  );
} else {
  console.log("check:i18n: no unreferenced keys");
}
