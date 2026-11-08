//! Batch landed-cost allocation in integer cents (ARCHITECTURE §3.1).

use super::discount::apportion;
use super::money::Money;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostAllocation {
    EvenPerUnit,
    ByRetailValue,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocationLine {
    pub qty: i64,
    pub retail_price: Money,
    /// Only used by `Manual`.
    pub manual_unit_cost: Option<Money>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allocation {
    pub unit_costs: Vec<Money>,
    /// landed − Σ(unit_cost × qty). For even/by-value: 0 ≤ v < Σqty. Manual: anything.
    pub variance: Money,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AllocationError {
    #[error("landed cost must not be negative")]
    NegativeLanded,
    #[error("line {0}: quantity must not be negative")]
    NegativeQty(usize),
    #[error("line {0}: manual unit cost missing or negative")]
    BadManualCost(usize),
    #[error("nothing to allocate cost against (all quantities/weights are zero)")]
    NoWeight,
}

pub fn allocate(landed: Money, method: CostAllocation, lines: &[AllocationLine]) -> Result<Allocation, AllocationError> {
    if landed < 0 {
        return Err(AllocationError::NegativeLanded);
    }
    if let Some(i) = lines.iter().position(|l| l.qty < 0) {
        return Err(AllocationError::NegativeQty(i));
    }
    let unit_costs: Vec<Money> = match method {
        CostAllocation::Manual => lines
            .iter()
            .enumerate()
            .map(|(i, l)| match l.manual_unit_cost {
                Some(c) if c >= 0 => Ok(c),
                _ => Err(AllocationError::BadManualCost(i)),
            })
            .collect::<Result<_, _>>()?,
        CostAllocation::EvenPerUnit | CostAllocation::ByRetailValue => {
            let weights: Vec<Money> = lines
                .iter()
                .map(|l| match method {
                    CostAllocation::EvenPerUnit => l.qty,
                    _ => l.qty * l.retail_price,
                })
                .collect();
            let line_totals = apportion(landed, &weights).ok_or(AllocationError::NoWeight)?;
            lines
                .iter()
                .zip(line_totals)
                .map(|(l, total)| if l.qty == 0 { 0 } else { total / l.qty })
                .collect()
        }
    };
    let allocated: Money = lines.iter().zip(&unit_costs).map(|(l, c)| l.qty * c).sum();
    Ok(Allocation { unit_costs, variance: landed - allocated })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(qty: i64, retail: Money) -> AllocationLine {
        AllocationLine { qty, retail_price: retail, manual_unit_cost: None }
    }

    #[test]
    fn even_split_with_variance() {
        // Rs 10.00 over 3 units ⇒ 333 each, 1 cent variance.
        let a = allocate(1_000, CostAllocation::EvenPerUnit, &[l(3, 0)]).unwrap();
        assert_eq!(a.unit_costs, vec![333]);
        assert_eq!(a.variance, 1);
    }

    #[test]
    fn by_retail_value_weights_expensive_items() {
        // Shoe batch: Rs 60,000 landed; 20 pairs @ Rs 4,500 retail, 10 pairs @ Rs 1,500 retail.
        let a = allocate(6_000_000, CostAllocation::ByRetailValue, &[l(20, 450_000), l(10, 150_000)]).unwrap();
        // weights 9,000,000 : 1,500,000 ⇒ line totals 5,142,857 / 857,143
        assert_eq!(a.unit_costs, vec![257_142, 85_714]);
        assert_eq!(a.variance, 6_000_000 - (20 * 257_142 + 10 * 85_714));
        assert!(a.variance >= 0 && a.variance < 30);
    }

    #[test]
    fn variance_bound_holds_for_many_inputs() {
        for landed in [0, 1, 999, 1_000_001, 7_777_777] {
            for lines in [vec![l(3, 100), l(7, 250)], vec![l(1, 1)], vec![l(13, 99), l(0, 5), l(2, 10_000)]] {
                for m in [CostAllocation::EvenPerUnit, CostAllocation::ByRetailValue] {
                    let a = allocate(landed, m, &lines).unwrap();
                    let units: i64 = lines.iter().map(|x| x.qty).sum();
                    assert!(a.variance >= 0 && a.variance < units.max(1), "{landed} {m:?} {lines:?} → {a:?}");
                }
            }
        }
    }

    #[test]
    fn manual_reports_live_variance() {
        let lines = vec![
            AllocationLine { qty: 2, retail_price: 0, manual_unit_cost: Some(400) },
            AllocationLine { qty: 1, retail_price: 0, manual_unit_cost: Some(150) },
        ];
        assert_eq!(allocate(1_000, CostAllocation::Manual, &lines).unwrap().variance, 50);
        let missing = vec![AllocationLine { qty: 1, retail_price: 0, manual_unit_cost: None }];
        assert_eq!(allocate(1, CostAllocation::Manual, &missing), Err(AllocationError::BadManualCost(0)));
    }

    #[test]
    fn no_weight_is_an_error() {
        assert_eq!(allocate(100, CostAllocation::ByRetailValue, &[l(3, 0)]), Err(AllocationError::NoWeight));
    }
}
