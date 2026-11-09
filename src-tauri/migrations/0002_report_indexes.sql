-- GayanShop POS — schema v4.2 (Block 2.10): an index for the P&L's stock write-offs (doc §5.3, §13.4).
-- ix_mov_date_reason (business_date, reason) walks every movement of the period to find the few write-offs
-- among the sale slices: a yearly P&L at 1,000,000 order items read a year of slices (≈ 140 ms). This partial
-- index holds only write-off movements, so the same query reads a few hundred entries. The P&L query repeats
-- the exact reason list, which is what lets SQLite use a partial index.
CREATE INDEX ix_mov_writeoffs ON stock_movements(business_date)
  WHERE reason IN ('damage','count_correction','adjustment');
