// Fails the build if production sources contain placeholders or debug leftovers (AGENT.md §2 "Quality").
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const root = process.cwd();

const RULES = [
  {
    dir: "src",
    ext: /\.(ts|tsx)$/,
    skip: /(\.test\.(ts|tsx)$|[\\/]db[\\/]bindings\.ts$)/,
    bans: [/\bTODO\b/, /\bFIXME\b/, /console\.log\(/, /\bdebugger\b/],
  },
  {
    dir: "src-tauri/src",
    ext: /\.rs$/,
    skip: null,
    bans: [/\btodo!\(/, /\bunimplemented!\(/, /\bdbg!\(/, /\bTODO\b/, /\bFIXME\b/],
  },
  { dir: "src-tauri/migrations", ext: /\.sql$/, skip: null, bans: [/\bTODO\b/, /\bFIXME\b/] },
];

function* walk(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) yield* walk(path);
    else yield path;
  }
}

const findings = [];
for (const rule of RULES) {
  const base = join(root, rule.dir);
  let exists = true;
  try {
    statSync(base);
  } catch {
    exists = false;
  }
  if (!exists) continue;
  for (const file of walk(base)) {
    if (!rule.ext.test(file) || (rule.skip && rule.skip.test(file))) continue;
    readFileSync(file, "utf8")
      .split(/\r?\n/)
      .forEach((line, i) => {
        for (const ban of rule.bans) {
          if (ban.test(line)) findings.push(`${relative(root, file)}:${i + 1}: ${ban} → ${line.trim()}`);
        }
      });
  }
}

if (findings.length > 0) {
  process.stderr.write(`guard: ${findings.length} banned pattern(s) found:\n${findings.join("\n")}\n`);
  process.exit(1);
}
process.stdout.write("guard: clean\n");
