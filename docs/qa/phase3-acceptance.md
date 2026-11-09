# Phase 3 acceptance: cloud sync

> Block 3.6 gate (AGENT.md §5). Each item shows **how it was verified**:
> - **Chaos**: `cargo test --manifest-path src-tauri/Cargo.toml --test chaos -- --ignored`. Three terminal databases run the real checkout, void and back-office code. They sync with the real local Supabase stack (Postgres, PostgREST, GoTrue, the `register-device` function) through toxiproxy, which cuts the network on demand. Each counter is registered the way the owner would do it. The cloud's own views are read back with psql.
> - **pgTAP**: `pnpm exec supabase test db` covers the server's rules: RLS, idempotency, merge rules, invariants, the parent closure and registration.
> - **Automated**: a named Rust or Vitest test that runs in `pnpm verify`.
> - **Owner**: needs the owner's Supabase account, the live project or the counter PCs.
>
> Nothing in this phase touched the live project (`tqfoinnftjxlueutlufq`). Every test runs against the local stack on this machine, and the chaos suite refuses any address other than 127.0.0.1.

## Running the suites

```bash
export PATH="$HOME/.cargo/bin:$PATH"
pnpm exec supabase start -x realtime,storage-api,imgproxy,mailpit,postgres-meta,studio,logflare,vector,supavisor
pnpm exec supabase test db                                                   # pgTAP
cargo test --manifest-path src-tauri/Cargo.toml --test chaos -- --ignored    # starts toxiproxy (Docker) itself
pnpm exec supabase stop
```

The chaos suite reads the local stack's address and keys from `supabase status` at run time, and never writes them anywhere. Tests take turns on the one proxy.

## 1. Doc §8 Phase 3 exit criteria

| # | Exit criterion | Verified by | Result |
|---|---|---|---|
| 3.1 | 200 offline sales per terminal, then reconnect: cloud and every terminal show identical daily totals, batch reports, stock valuation and customer stats | Chaos `two_hundred_offline_sales_per_terminal_reconcile`. Three counters each sell 200 while toxiproxy is down; a round fails and keeps everything queued. After reconnecting, every terminal and the cloud agree: daily sales per counter, stock levels, layer stock and sales, customer stats. Every synced row is byte-identical on all three terminals. Automated: `sync_apply.rs › three_counters_sell_offline_and_converge`. | ✅ 2026-11-09 |
| 3.2 | Kill the network mid-push: no duplicates and no losses | Chaos `killing_the_network_mid_push_neither_duplicates_nor_loses`. The push reaches the server but the answers are dropped (toxiproxy `timeout`). The client times out, and nothing leaves the outbox. The retry is answered from `processed_ops`, so the cloud holds exactly 40 orders, 40 payments and every audit row once. Automated: `sync_apply.rs › losing_the_network_mid_push…`; pgTAP idempotency. | ✅ 2026-11-09 |
| 3.3 | Two terminals selling the last unit of a layer: both sales are kept, the layer shows −1, and it's reported | Chaos `three_counters_oversell_last_units` (doc §13.1). A layer of 2 sells 4 offline across three counters: the ledger is identical on all three nodes and the cloud, the layer is −2 and flagged, and the next sale uses the next layer. | ✅ 2026-11-09 |
| 3.4 | The same new phone number captured on T01 and T02 while offline: one customer after sync, with both orders in their history | Chaos `a_void_and_a_shared_phone_reach_every_counter` (and 8 shared phones in 3.1). Automated: `sync_apply.rs`. | ✅ 2026-11-09 |
| 3.5 | A void on T02 (in its open session) appears as voided on T01 and T03 after sync, and their store-wide reports exclude it | Chaos `a_void_and_a_shared_phone_reach_every_counter`. Automated: `sync_apply.rs › a_void_reaches_the_other_counters`. | ✅ 2026-11-09 |
| 3.6 | A ±1-day clock skew on one terminal changes nothing | Chaos `a_day_of_clock_skew_on_one_counter_changes_nothing`. T03 runs a day ahead, and the cloud and every terminal still agree row for row: ordering is by `server_seq`, never by a device's clock. | ✅ 2026-11-09 |
| 3.7 | Bootstrap with `limit = 7`, where batches and batch items were edited after their stock movements and orders were voided after their payments: no foreign-key aborts, and the derived tables equal `rebuild_derived` output | Chaos `bootstrap_in_pages_of_seven_after_parents_were_edited` against the real server. Automated: `bootstrap.rs` (3), including proof that without the parent closure the same bootstrap fails. | ✅ 2026-11-09 |
| 3.8 | Concurrent owner demotions are rejected by the invariant | Chaos `concurrent_owner_demotions_are_rejected_by_the_invariant`. Each counter demotes a different Owner offline. The second demotion to arrive is refused (`OWNER_INVARIANT`, shown as a refused change on that counter). The cloud's version then replaces it there, so every node keeps exactly one active Owner. pgTAP covers the invariant too. | ✅ 2026-11-09 |
| 3.9 | Editing an audit row in a terminal's DB file is flagged by chain verification | Chaos `editing_an_audit_row_in_a_terminal_file_is_flagged`. T01's own verification flags the edited entry. The forgery doesn't spread: the outbox carries each entry as it was written, so the cloud and the other counters keep the true row and its chain verifies. A pushed chain that doesn't link is flagged server-side (`devices.chain_broken_at`, pgTAP) and shown in Settings → Sync. | ✅ 2026-11-09 |

## 2. Defects the chaos suite found (all fixed)

Migration `20261109000500_chaos_fixes.sql` and the client:
- **Same-millisecond writes.** A batch created and received in one transaction reached the cloud as two snapshots with the same `(updated_at, device)`, and the second was dropped as stale. Now the later snapshot from the same device wins the tie, on both server and client.
- **Checkout order.** Checkout queued an order's lines, payments and slices before the sealed order. Children reached the cloud before their parent and had to retry. Checkout now moves them behind the order.
- **Refused changes.** A refused change (for example an Owner demotion) stayed on the counter that made it, which then disagreed with the cloud. Now the cloud re-sends its row, and it replaces the refused change.
- **Checkout tokens.** They were unique across all stores in the cloud; now they're unique per store.

Found earlier in the phase:
- **Erasure.** An erasure older than a concurrent edit was dropped as stale (`c965ac4`).
- **Purchase dates.** A batch's purchase date wasn't stored in the canonical UTC form (`51231db`).

## 3. Owner steps (not done by the agent: they need your accounts)

1. **Log in with the Supabase CLI.** Run `pnpm exec supabase login` (it opens the browser), then `pnpm exec supabase link --project-ref tqfoinnftjxlueutlufq`. Linking asks for the database password.
2. **Push the schema.** `pnpm exec supabase db push` applies `supabase/migrations` to the live project, including Block 4.1's `20261110000100_health_audit_head.sql` (restore recovery reads the device's audit head from `sync_health`). Review the plan it prints first.
3. **Deploy the function.** `pnpm exec supabase functions deploy register-device`. The function reads `SUPABASE_URL`, `SUPABASE_ANON_KEY` and `SUPABASE_SERVICE_ROLE_KEY`, which Supabase provides to functions automatically. The service-role key never goes into the app.
   - If registering a counter then fails with "The server's registration service isn't set up yet" (`FUNCTION_NOT_CONFIGURED`), those variables are missing in the project's function settings.
   - Before `db push`, check that the project's Postgres major version is 17, the same as `supabase/config.toml`.
4. **Turn sign-ups off.** In the dashboard's Authentication settings, turn off "Allow new users to sign up" (decision #19). The local stack already has `enable_signup = false`.
5. **Create the owner account.** Dashboard → Authentication → Users → Add user, with your email and a strong password. Tick "Auto confirm".
6. **Register T01.** On T01: Settings → Cloud sync → Register this counter.
   - Server address: `https://tqfoinnftjxlueutlufq.supabase.co`
   - Project key: the anon key
   - Then enter the owner email and password. T01 keeps its store, device id and code, and starts sending its history.
7. **Join T02 and T03.** On each new counter's first run: Choose a language → Join an existing store → the same four fields. Wait for "Bringing in the store's history" to finish, then log in with your existing PINs.
8. **Re-run the chaos suite on your machine** if you want to see it yourself. Docker Desktop must be running; see the commands above.

The server address and the anon key are kept per counter in its settings. Each counter's own sign-in token is kept in the Windows Credential Manager (`com.gayanshop.pos`). The owner's password is never stored. Moving to your own VPS later is a change of server address and key on each counter: the protocol is plain HTTP + JSON against PostgREST and GoTrue.

## 4. Manual — pending (counter PCs)

| Item | Result |
|---|---|
| Register T01 against the live project, then join T02 and T03 (steps 6–7) | ☐ |
| Unplug a counter's network for a few sales, then plug it back in: the pill goes red (offline, N queued), then green, and the other counters show the sales | ☐ |
| Settings → Cloud sync in Sinhala, reviewed by a native speaker | ☐ |
