-- GayanShop POS: schema v4.3. Several active products may share one barcode.
-- Manufacturers sometimes print the same barcode on different variants (flavours, scents, sizes), and
-- re-labelling every unit is impractical. The till resolves a shared barcode with a "Select product" dialog.
-- The partial UNIQUE index becomes a plain index of the same shape, so the scanner lookup stays indexed
-- (doc §13.4). SKUs stay unique; so does a product's own in-store generated barcode (the generator skips
-- codes already in use).
DROP INDEX ux_products_barcode;
CREATE INDEX idx_products_barcode ON products(store_id, barcode)
  WHERE barcode IS NOT NULL AND deleted_at IS NULL AND is_active = 1;
