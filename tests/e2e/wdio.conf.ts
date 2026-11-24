// WebdriverIO + tauri-driver (Block 1.12): drives the real release binary through WebView2.
// Needs `tauri-driver` (cargo install tauri-driver --locked) and an `msedgedriver` matching the installed
// WebView2 runtime, both on PATH or in ~/.cargo/bin. Every run gets a fresh, throwaway data folder through
// GAYANSHOP_DATA_DIR, so the shop's real database is never touched. Phase 1 starts from an empty folder (first
// run); Phase 2 starts from a copy of the §13.4 gate dataset, written once per run by
// src-tauri/tests/e2e_dataset.rs.
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { browser } from "@wdio/globals";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const exe = (name: string) => (process.platform === "win32" ? `${name}.exe` : name);
const application = path.join(root, "src-tauri", "target", "release", exe("gayanshop-pos"));
const cargoBin = path.join(os.homedir(), ".cargo", "bin");
const scaleDir = path.join(os.tmpdir(), "gayanshop-e2e-scale");

function tool(name: string): string {
  const local = path.join(cargoBin, exe(name));
  return fs.existsSync(local) ? local : exe(name);
}

/** tauri-driver needs a moment before it accepts the session. */
async function waitForPort(port: number, ms = 15_000): Promise<void> {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    const open = await new Promise<boolean>((resolve) => {
      const socket = net.connect(port, "127.0.0.1", () => {
        socket.end();
        resolve(true);
      });
      socket.on("error", () => resolve(false));
    });
    if (open) return;
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`tauri-driver did not listen on ${port}`);
}

let driver: ChildProcess | null = null;
let dataDir = "";

export const config: WebdriverIO.Config = {
  runner: "local",
  specs: ["./phase1.e2e.ts", "./phase2.e2e.ts"],
  maxInstances: 1,
  hostname: "127.0.0.1",
  port: 4444,
  capabilities: [{ maxInstances: 1, "tauri:options": { application } } as WebdriverIO.Capabilities],
  logLevel: "warn",
  waitforTimeout: 15_000,
  reporters: ["spec"],
  framework: "mocha",
  mochaOpts: { ui: "bdd", timeout: 180_000, bail: true },

  /**
   * The dataset first, then the app: `cargo test --release` also rebuilds the package's binary into
   * target/release without Tauri's `custom-protocol` feature (a dev build that loads localhost), so the app
   * must be built after it. E2E_SKIP_BUILD=1 reuses both only when neither had to be rebuilt.
   */
  onPrepare() {
    const reuse = process.env.E2E_SKIP_BUILD === "1";
    let seeded = false;
    if (!(reuse && fs.existsSync(path.join(scaleDir, "gayanshop_pos.db")))) {
      fs.rmSync(scaleDir, { recursive: true, force: true });
      const seed = spawnSync(
        tool("cargo"),
        [
          "test",
          "--manifest-path",
          "src-tauri/Cargo.toml",
          "--release",
          "--test",
          "e2e_dataset",
          "--",
          "--ignored",
        ],
        { cwd: root, stdio: "inherit", env: { ...process.env, GAYANSHOP_E2E_SCALE_DIR: scaleDir } },
      );
      if (seed.status !== 0) throw new Error(`e2e dataset failed (${seed.status})`);
      seeded = true;
    }
    if (reuse && !seeded && fs.existsSync(application)) return;
    const build = spawnSync("pnpm", ["tauri", "build", "--no-bundle"], {
      cwd: root,
      stdio: "inherit",
      shell: true,
    });
    if (build.status !== 0) throw new Error(`pnpm tauri build --no-bundle failed (${build.status})`);
  },

  async beforeSession(_config, _capabilities, specs) {
    dataDir = fs.mkdtempSync(path.join(os.tmpdir(), "gayanshop-e2e-"));
    if (specs.some((s) => s.includes("phase2"))) fs.cpSync(scaleDir, dataDir, { recursive: true });
    driver = spawn(tool("tauri-driver"), ["--native-driver", tool("msedgedriver")], {
      stdio: [null, process.stdout, process.stderr],
      env: { ...process.env, GAYANSHOP_DATA_DIR: dataDir },
    });
    await waitForPort(4444);
  },

  /** A failing step leaves a screenshot next to the throwaway data folder (path printed). */
  async afterTest(test, _context, result) {
    if (result.passed) return;
    const file = path.join(os.tmpdir(), `gayanshop-e2e-${test.title.replace(/\W+/g, "-")}.png`);
    await browser.saveScreenshot(file);
    process.stderr.write(`screenshot: ${file}
`);
  },

  afterSession() {
    driver?.kill();
    driver = null;
    fs.rmSync(dataDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 });
  },
};
