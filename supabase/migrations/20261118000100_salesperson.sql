-- Mirror of SQLite migration 0004 (who made the sale): staff.staff_no and orders.sold_by.
-- staff_no is only indexed, not unique, here: two counters could each hand out the next free number while
-- offline, and a unique violation would wedge a terminal's push queue. The owner fixes a clash in Staff.
ALTER TABLE pos.staff ADD COLUMN staff_no integer CHECK (staff_no IS NULL OR staff_no > 0);
CREATE INDEX ix_staff_no ON pos.staff(store_id, staff_no) WHERE staff_no IS NOT NULL;
ALTER TABLE pos.orders ADD COLUMN sold_by uuid REFERENCES pos.staff(id);

CREATE OR REPLACE FUNCTION pos.orders_state_machine() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF NOT (OLD.status = 'completed' AND NEW.status = 'voided'
      AND NEW.id = OLD.id AND NEW.store_id = OLD.store_id AND NEW.device_id = OLD.device_id
      AND NEW.cash_session_id = OLD.cash_session_id
      AND NEW.receipt_no = OLD.receipt_no AND NEW.checkout_token = OLD.checkout_token
      AND NEW.staff_id = OLD.staff_id AND NEW.sold_by IS NOT DISTINCT FROM OLD.sold_by
      AND NEW.approved_by IS NOT DISTINCT FROM OLD.approved_by
      AND NEW.customer_id IS NOT DISTINCT FROM OLD.customer_id
      AND NEW.line_count = OLD.line_count AND NEW.item_count = OLD.item_count
      AND NEW.gross_sales = OLD.gross_sales AND NEW.discount_total = OLD.discount_total
      AND NEW.net_sales = OLD.net_sales AND NEW.tax_total = OLD.tax_total AND NEW.rounding = OLD.rounding
      AND NEW.grand_total = OLD.grand_total AND NEW.paid_total = OLD.paid_total AND NEW.change_due = OLD.change_due
      AND NEW.cost_total = OLD.cost_total AND NEW.quick_sale_net = OLD.quick_sale_net
      AND NEW.unknown_cost_net = OLD.unknown_cost_net
      AND NEW.business_date = OLD.business_date AND NEW.completed_at = OLD.completed_at
      AND NEW.updated_by_device = OLD.device_id) THEN
    RAISE EXCEPTION 'orders: illegal state transition or mutation of a sold order' USING ERRCODE = 'check_violation';
  END IF;
  RETURN NEW;
END $$;
