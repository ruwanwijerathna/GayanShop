-- GayanShop POS — schema v4.4: who made the sale.
-- Everyone who works in the shop is a staff row with a small number (1, 2, 3 …) the cashier types at the till
-- (`staff.staff_no`, auto-assigned, never reused). The number typed for a sale is stored on the order as
-- `orders.sold_by` (the salesperson), separate from `orders.staff_id` (the logged-in cashier whose shift and
-- drawer the sale belongs to), so several people can share one till login and still be credited correctly.
-- Orders sold before this migration keep `sold_by` NULL; reports read COALESCE(sold_by, staff_id).
ALTER TABLE staff ADD COLUMN staff_no INTEGER CHECK (staff_no IS NULL OR staff_no > 0);
UPDATE staff SET staff_no = (
  SELECT COUNT(*) FROM staff s2
  WHERE s2.store_id = staff.store_id AND (s2.created_at < staff.created_at OR (s2.created_at = staff.created_at AND s2.id <= staff.id))
);
CREATE UNIQUE INDEX ux_staff_no ON staff(store_id, staff_no) WHERE staff_no IS NOT NULL;

ALTER TABLE orders ADD COLUMN sold_by TEXT REFERENCES staff(id);
CREATE INDEX ix_orders_sold_by ON orders(COALESCE(sold_by, staff_id), business_date) WHERE status = 'completed';

-- The salesperson is part of "who sold it": frozen once the order exists, like staff_id.
DROP TRIGGER trg_orders_state_machine;
CREATE TRIGGER trg_orders_state_machine BEFORE UPDATE ON orders
WHEN NOT (
      ((OLD.status = 'pending' AND NEW.status = 'completed') OR (OLD.status = 'completed' AND NEW.status = 'voided'))
  AND NEW.id = OLD.id AND NEW.device_id = OLD.device_id AND NEW.cash_session_id = OLD.cash_session_id
  AND NEW.receipt_no = OLD.receipt_no AND NEW.checkout_token = OLD.checkout_token
  AND NEW.staff_id = OLD.staff_id AND NEW.sold_by IS OLD.sold_by
  AND NEW.approved_by IS OLD.approved_by AND NEW.customer_id IS OLD.customer_id
  AND NEW.line_count = OLD.line_count AND NEW.item_count = OLD.item_count
  AND NEW.gross_sales = OLD.gross_sales AND NEW.discount_total = OLD.discount_total
  AND NEW.net_sales = OLD.net_sales AND NEW.tax_total = OLD.tax_total AND NEW.rounding = OLD.rounding
  AND NEW.grand_total = OLD.grand_total AND NEW.paid_total = OLD.paid_total AND NEW.change_due = OLD.change_due
  AND NEW.cost_total = OLD.cost_total AND NEW.quick_sale_net = OLD.quick_sale_net
  AND NEW.unknown_cost_net = OLD.unknown_cost_net
  AND NEW.business_date = OLD.business_date AND NEW.completed_at = OLD.completed_at
)
BEGIN SELECT RAISE(ABORT, 'orders: illegal state transition or mutation of a sold order'); END;
