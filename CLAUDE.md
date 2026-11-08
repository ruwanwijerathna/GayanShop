# CLAUDE.md: GayanShop POS

@AGENT.md

The import above loads the full operating manual: identity, non-negotiable rules, the execution protocol, git rules, the Atomic Block roadmap with gates, and the commands. **It is binding.** Design truth is `docs/ARCHITECTURE.md`; read the sections a block cites before starting it.

## Claude Code–specific rules

1. **Phase 0 is ON HOLD.**
   - Don't write application code, scaffold or install packages until the owner explicitly says to start, naming a phase or block.
   - Git is already initialised (`main`, root commit `ca36dcc`).
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
   - The message ends with the trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
   - Never `--amend` a committed block, never `--no-verify`, never push unless asked.
   - The block's `[x]` tick in `AGENT.md` §5 goes in the same commit.
5. **Stop and ask** on:
   - Spec ambiguity
   - Any architecture or policy change
   - Anything needing the owner's accounts, credentials, purchases or hardware (Supabase, GitHub remote, code-signing, printers)
   - 3 failed fix attempts on the same gate failure

   Propose doc edits; don't silently diverge from `docs/ARCHITECTURE.md`.
6. **Pre-v4 drafts** already in `src-tauri/` are not trustworthy (see `AGENT.md` §3.8). Review and rework them inside the block that owns them. Never wire them in as-is.
7. **Context resets:** resume from the first unticked block in `AGENT.md` §5, after checking the last commit. The roadmap checkboxes plus `git log` are the progress record.
