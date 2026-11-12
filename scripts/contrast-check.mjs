// Asserts the WCAG 2.x contrast ratios promised in docs/ARCHITECTURE.md §1.9 for BOTH themes, reading the
// actual token values from src/styles/globals.css.
import { readFileSync } from "node:fs";
import { join } from "node:path";

const css = readFileSync(join(process.cwd(), "src", "styles", "globals.css"), "utf8");

function block(selector) {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`token block ${selector} not found`);
  const body = css.slice(start, css.indexOf("}", start));
  const vars = {};
  for (const m of body.matchAll(/--([a-z0-9-]+):\s*(#[0-9a-fA-F]{6})\s*;/g)) vars[m[1]] = m[2];
  return vars;
}

function luminance(hex) {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = Number.parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function ratio(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

// [foreground token, background token, minimum ratio, why]
const RULES = [
  ["foreground", "background", 7, "till/body text ≥ 7:1 (AAA, glare)"],
  ["card-foreground", "card", 7, "panel text ≥ 7:1"],
  ["muted-foreground", "background", 7, "secondary text ≥ 7:1 on background"],
  ["muted-foreground", "muted", 4.5, "secondary text on muted surfaces"],
  ["primary-foreground", "primary", 4.5, "Pay / primary buttons"],
  ["destructive-foreground", "destructive", 4.5, "void / error buttons"],
  ["warning-foreground", "warning", 4.5, "warning banners"],
  ["info-foreground", "info", 4.5, "info notices"],
  ["success-foreground", "success-surface", 4.5, "sale-complete banner"],
  ["secondary-foreground", "secondary", 4.5, "secondary buttons"],
  ["destructive", "background", 4.5, "error / shortfall text on the page"],
  ["destructive", "card", 4.5, "error / shortfall text on panels"],
  ["success-foreground", "background", 4.5, "OK / balanced text on the page"],
  ["success-foreground", "card", 4.5, "OK / balanced text on panels"],
  ["input", "background", 3, "control boundaries (WCAG 1.4.11)"],
  ["input", "card", 3, "control boundaries on panels"],
  ["ring", "background", 3, "focus ring"],
  ["primary", "background", 3, "primary button boundary"],
  ["sidebar-foreground", "sidebar", 7, "sidebar navigation text"],
  ["sidebar-accent-foreground", "sidebar-accent", 7, "sidebar hovered item"],
  ["sidebar-primary-foreground", "sidebar-primary", 4.5, "sidebar active item"],
  ["sidebar-ring", "sidebar", 3, "sidebar focus ring"],
];

const failures = [];
let checked = 0;
for (const [theme, selector] of [
  ["light", ":root"],
  ["dark", ".dark"],
]) {
  const t = block(selector);
  for (const [fg, bg, min, why] of RULES) {
    if (!t[fg] || !t[bg]) {
      failures.push(`${theme}: missing token --${fg} or --${bg}`);
      continue;
    }
    const r = ratio(t[fg], t[bg]);
    checked += 1;
    if (r < min) failures.push(`${theme}: --${fg} on --${bg} = ${r.toFixed(2)}:1 < ${min}:1 (${why})`);
  }
}

if (failures.length > 0) {
  process.stderr.write(`contrast:check: ${failures.length} failure(s):\n${failures.join("\n")}\n`);
  process.exit(1);
}
process.stdout.write(
  `contrast:check: ${checked} token pairs meet their minimum ratio in light and dark themes\n`,
);
