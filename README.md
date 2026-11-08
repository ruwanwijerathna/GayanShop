# GayanShop POS

Offline-first desktop point of sale for GayanShop — Tauri v2 (Rust) + React 19 + TypeScript + Tailwind CSS v4 + shadcn/ui, with a local SQLite source of truth and (later) Supabase sync.

- **Architecture & policies:** [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- **Engineering rules, roadmap & verification gates:** [`AGENT.md`](AGENT.md)

```bash
pnpm install
pnpm tauri dev     # run the desktop app
pnpm verify        # full quality gate (lint, types, tests, i18n, guard, rustfmt, clippy, cargo test)
```

On Windows Git Bash, put cargo on PATH first: `export PATH="$HOME/.cargo/bin:$PATH"`.
