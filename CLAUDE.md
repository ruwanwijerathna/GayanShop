# CLAUDE.md: GayanShop POS

@AGENT.md

The import above loads the full operating manual: identity, non-negotiable rules, the execution protocol, git rules, the Atomic Block roadmap with gates, and the commands. **It is binding.** Design truth is `docs/ARCHITECTURE.md`; read the sections a block cites before starting it.

## Claude Code–specific rules

1. **Phases 0, 1 and 2 are complete and merged to `main`. Phase 3 (cloud sync) is in progress** on branch `feat/phase-3-cloud-sync`.
   - Work only on blocks the owner has explicitly named. Don't start the next block or phase until they say so.
   - "Explain", "plan" or "review" is not a start command.
2. **Work in roadmap order, one block per turn sequence.** Before each block:
   - Read `AGENT.md` §5 for that block, and the doc sections it cites.
   - Check `git status` and `git log --oneline -5`.
   - Track the block's deliverables as a todo list.
3. **Gates are real or they didn't happen.**
   - Run every gate command yourself, in Git Bash, after `export PATH="$HOME/.cargo/bin:$PATH"`, from the quoted repo root.
   - Report the actual result. If anything is red, say so with the output.
   - Never tick a block, commit, or move on with a red or unrun gate.
   - Run long builds (first `cargo build`, `pnpm tauri build`) in the background and wait for the completion notification. Don't poll.
4. **Commit exactly once per green block**, using the `AGENT.md` §4 convention: `feat(block-X.Y): …` / `test(block-X.Y): …`.
   - The message body lists the deliverables and the gate commands that passed.
   - The message ends with the attribution trailer(s) the current session's instructions specify (`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, plus a `Claude-Session:` line when given).
   - Never `--amend` a committed block, never `--no-verify`, never push unless asked.
   - The block's `[x]` tick in `AGENT.md` §5 goes in the same commit.
5. **Stop and ask** on:
   - Spec ambiguity
   - Any architecture or policy change
   - Anything needing the owner's accounts, credentials, purchases or hardware (Supabase, GitHub remote, code-signing, printers)
   - 3 failed fix attempts on the same gate failure

   Propose doc edits; don't silently diverge from `docs/ARCHITECTURE.md`.
6. **Windows gotchas learned in Phase 0:**
   - Long scripted edits through Bash heredocs can be mangled by the shell, so prefer the Write/Edit tools.
   - `cargo fetch` can need `CARGO_NET_RETRY=10 CARGO_HTTP_TIMEOUT=180` on this network.
   - Regenerate and **stage** `src/db/bindings.ts` before `pnpm verify` when commands change. `bindings:check` diffs the working tree against the index.
7. **Context resets:** resume from the first unticked block in `AGENT.md` §5, after checking the last commit. The roadmap checkboxes plus `git log` are the progress record.
