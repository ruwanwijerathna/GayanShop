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
    #[error("line {0}: retail price must not be negative")]
    NegativeRetail(usize),
    #[error("line {0}: manual unit cost missing or negative")]
    BadManualCost(usize),
    #[error("nothing to allocate cost against (all quantities/weights are zero)")]
    NoWeight,
    #[error("amount too large")]
    Overflow,
}

pub fn allocate(
    landed: Money,
    method: CostAllocation,
    lines: &[AllocationLine],
) -> Result<Allocation, AllocationError> {
    if landed < 0 {
        return Err(AllocationError::NegativeLanded);
    }
    for (i, l) in lines.iter().enumerate() {
        if l.qty < 0 {
            return Err(AllocationError::NegativeQty(i));
        }
        if l.retail_price < 0 {
            return Err(AllocationError::NegativeRetail(i));
        }
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
            let weights = lines
                .iter()
                .map(|l| match method {
                    CostAllocation::EvenPerUnit => Some(l.qty),
                    _ => l.qty.checked_mul(l.retail_price),
                })
                .collect::<Option<Vec<_>>>()
                .ok_or(AllocationError::Overflow)?;
            let line_totals = apportion(landed, &weights).ok_or(AllocationError::NoWeight)?;
            lines.iter().zip(line_totals).map(|(l, total)| if l.qty == 0 { 0 } else { total / l.qty }).collect()
        }
    };
    let allocated = lines
        .iter()
        .zip(&unit_costs)
        .try_fold(0i64, |acc, (l, c)| l.qty.checked_mul(*c).and_then(|v| acc.checked_add(v)))
        .ok_or(AllocationError::Overflow)?;
    Ok(Allocation { unit_costs, variance: landed - allocated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn l(qty: i64, retail: Money) -> AllocationLine {
        AllocationLine { qty, retail_price: retail, manual_unit_cost: None }
    }

    #[test]
    fn even_split_with_variance() {
        let a = allocate(1_000, CostAllocation::EvenPerUnit, &[l(3, 0)]).unwrap();
        assert_eq!((a.unit_costs, a.variance), (vec![333], 1));
    }

    #[test]
    fn by_retail_value_weights_expensive_items() {
        // Shoe batch: Rs 60,000 landed; 20 pairs @ Rs 4,500 retail, 10 pairs @ Rs 1,500 retail.
        let a = allocate(6_000_000, CostAllocation::ByRetailValue, &[l(20, 450_000), l(10, 150_000)]).unwrap();
        assert_eq!(a.unit_costs, vec![257_142, 85_714]);
        assert_eq!(a.variance, 20);
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
    fn errors() {
        assert_eq!(allocate(100, CostAllocation::ByRetailValue, &[l(3, 0)]), Err(AllocationError::NoWeight));
        assert_eq!(allocate(-1, CostAllocation::EvenPerUnit, &[l(1, 1)]), Err(AllocationError::NegativeLanded));
        assert_eq!(allocate(1, CostAllocation::EvenPerUnit, &[l(-1, 1)]), Err(AllocationError::NegativeQty(0)));
        assert_eq!(allocate(1, CostAllocation::ByRetailValue, &[l(i64::MAX, 2)]), Err(AllocationError::Overflow));
    }

    proptest! {
        #[test]
        fn variance_is_bounded(landed in 0i64..100_000_000, raw in prop::collection::vec((0i64..500, 0i64..2_000_000), 1..20), by_value in any::<bool>()) {
            let lines: Vec<AllocationLine> = raw.iter().map(|(q, r)| l(*q, *r)).collect();
            let method = if by_value { CostAllocation::ByRetailValue } else { CostAllocation::EvenPerUnit };
            match allocate(landed, method, &lines) {
                Ok(a) => {
                    let units: i64 = lines.iter().map(|x| x.qty).sum();
                    prop_assert!(a.variance >= 0 && a.variance < units.max(1));
                    prop_assert!(a.unit_costs.iter().all(|c| *c >= 0));
                }
                Err(e) => prop_assert_eq!(e, AllocationError::NoWeight),
            }
        }
    }
}
