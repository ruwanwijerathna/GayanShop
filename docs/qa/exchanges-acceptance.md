# Exchanges acceptance (owner checks, v4.5)

Automated coverage: `tests/exchange.rs`, `tests/exchange_policy.rs`, the pgTAP file `supabase/tests/04_exchanges.test.sql`, and the Vitest suites under `src/features/pos`. These checks need the real till, printer and Supabase stack.

## Before you start
- [ ] Deploy migration `20261120000100_exchanges.sql` (`supabase db push`) **before** updating the counters, so old and new terminals can both sync.
- [ ] Start each updated counter once. Confirm Settings shows the new receipt footer ("Exchange within 7 days. Item for item only; no cash refunds. Thank you, come again!") and the audit log has one `POLICY_UPGRADED` row. If you had edited the footer yourself, confirm your wording was kept.
- [ ] Confirm the footer wording and the rule "the replacement must be worth at least the returned goods" are what you want. Both are proposals.

## Till flow (run in English, then Sinhala)
- [ ] Sell two items (one with a line discount) and note the receipt number.
- [ ] Press **F12**, type the receipt number, return one unit. The credit equals what the customer paid for that unit (after discounts).
- [ ] Scan a replacement worth more. The banner shows credit and TO PAY. Pay the difference. Receipt prints EXCHANGE, the returned item with the original receipt number, the credit and TO PAY.
- [ ] Even swap: TO PAY is 0 and the sale completes with no payment dialog amount.
- [ ] Cheaper replacement: Pay is disabled with the "replacement too low" message.
- [ ] Hold and recall are blocked during an exchange; cancel the exchange returns to a normal cart.
- [ ] Quick-sale lines and a voided sale cannot be exchanged.
- [ ] Touch only: every stepper and button is easy to hit (48 px).

## Late exchange
- [ ] Look up a sale older than 7 days. The late warning shows. A cashier is asked for a Supervisor/Owner PIN at payment; the approval works once; a wrong PIN is refused.
- [ ] A sale exactly 7 days old (by business date) is not late; 8 days is.

## Stock and reports
- [ ] Returned units are available again in Stock and the batch's remaining quantity rises; the batch P&L shows the returned cost removed.
- [ ] Day's sales, discounts and the customer's total spend are net of the return.
- [ ] Z-report shows "Exchanges" and "Items returned" and the cash expected matches the drawer.
- [ ] An exchange order, and a sale that had goods returned, cannot be voided.

## Sync and hardware
- [ ] Exchange on counter 1; counter 2 shows the same stock and reports after sync. Repeat while counter 2 is offline.
- [ ] Real receipt printer prints the exchange receipt and cuts correctly; the drawer opens only if cash was taken.
