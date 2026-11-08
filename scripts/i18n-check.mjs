// Verifies that every i18n namespace and key exists in BOTH English and Sinhala (AGENT.md §2, doc §1.8).
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const LOCALES_DIR = join(process.cwd(), "src", "i18n", "locales");
const LANGS = ["en", "si"];

if (!existsSync(LOCALES_DIR)) {
  process.stdout.write(
    "i18n:check: no locales directory yet (src/i18n/locales is created in Block 0.8) — nothing to compare\n",
  );
  process.exit(0);
}

function flatten(obj, prefix = "") {
  return Object.entries(obj).flatMap(([k, v]) => {
    const key = prefix ? `${prefix}.${k}` : k;
    return v !== null && typeof v === "object" ? flatten(v, key) : [[key, v]];
  });
}

const problems = [];
const namespaces = new Set();
for (const lang of LANGS) {
  const dir = join(LOCALES_DIR, lang);
  if (!existsSync(dir)) {
    problems.push(`missing locale directory: ${lang}`);
    continue;
  }
  for (const f of readdirSync(dir)) if (f.endsWith(".json")) namespaces.add(f.replace(/\.json$/, ""));
}

for (const ns of [...namespaces].sort()) {
  const maps = {};
  for (const lang of LANGS) {
    const file = join(LOCALES_DIR, lang, `${ns}.json`);
    if (!existsSync(file)) {
      problems.push(`${lang}/${ns}.json is missing`);
      continue;
    }
    let parsed;
    try {
      parsed = JSON.parse(readFileSync(file, "utf8"));
    } catch (e) {
      problems.push(`${lang}/${ns}.json is not valid JSON: ${e.message}`);
      continue;
    }
    maps[lang] = new Map(flatten(parsed));
    for (const [key, value] of maps[lang]) {
      if (typeof value !== "string" || value.trim() === "")
        problems.push(`${lang}/${ns}.json: "${key}" is empty or not a string`);
    }
  }
  if (maps.en && maps.si) {
    for (const key of maps.en.keys())
      if (!maps.si.has(key)) problems.push(`si/${ns}.json is missing "${key}"`);
    for (const key of maps.si.keys())
      if (!maps.en.has(key)) problems.push(`en/${ns}.json is missing "${key}"`);
  }
}

if (problems.length > 0) {
  process.stderr.write(`i18n:check: ${problems.length} problem(s):\n${problems.join("\n")}\n`);
  process.exit(1);
}
process.stdout.write(`i18n:check: ${namespaces.size} namespace(s) in parity across ${LANGS.join(", ")}\n`);
