//! FIFO cost-layer consumption for sales (ARCHITECTURE §3.1, §13.1).

use super::money::Money;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub batch_item_id: String,
    pub on_hand: i64,
    pub unit_cost: Money,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slice {
    pub batch_item_id: String,
    pub qty: i64,
    pub unit_cost: Money,
    pub revenue_net: Money,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FifoError {
    #[error("quantity must be positive")]
    NonPositiveQty,
    #[error("revenue must not be negative")]
    NegativeRevenue,
    #[error("product has no cost layer; receive it through a batch first")]
    NoLayers,
    #[error("amount too large")]
    Overflow,
}

/// Consume `qty` units from `layers` (ordered oldest → newest by batch receive time).
/// Oldest layers with positive stock go first; anything left over is drawn from the newest layer,
/// which goes negative (oversold — allowed offline, flagged later). `revenue_net` is split by qty,
/// with the remainder on the last slice, so slices always sum to the line exactly.
pub fn consume(layers: &[Layer], qty: i64, revenue_net: Money) -> Result<Vec<Slice>, FifoError> {
    if qty <= 0 {
        return Err(FifoError::NonPositiveQty);
    }
    if revenue_net < 0 {
        return Err(FifoError::NegativeRevenue);
    }
    let newest = layers.last().ok_or(FifoError::NoLayers)?;
    let mut remaining = qty;
    let mut picks: Vec<(&str, i64, Money)> = Vec::new();
    for layer in layers {
        if remaining == 0 {
            break;
        }
        if layer.on_hand > 0 {
            let take = remaining.min(layer.on_hand);
            picks.push((&layer.batch_item_id, take, layer.unit_cost));
            remaining -= take;
        }
    }
    if remaining > 0 {
        match picks.last_mut() {
            Some(last) if last.0 == newest.batch_item_id => last.1 += remaining,
            _ => picks.push((&newest.batch_item_id, remaining, newest.unit_cost)),
        }
    }
    let n = picks.len();
    let mut slices = Vec::with_capacity(n);
    let mut allocated: Money = 0;
    for (i, (id, q, cost)) in picks.into_iter().enumerate() {
        let rev = if i + 1 == n {
            revenue_net - allocated
        } else {
            i64::try_from(i128::from(revenue_net) * i128::from(q) / i128::from(qty)).map_err(|_| FifoError::Overflow)?
        };
        allocated += rev;
        slices.push(Slice { batch_item_id: id.to_owned(), qty: q, unit_cost: cost, revenue_net: rev });
    }
    Ok(slices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn layer(id: &str, on_hand: i64, cost: Money) -> Layer {
        Layer { batch_item_id: id.into(), on_hand, unit_cost: cost }
    }

    #[test]
    fn takes_oldest_first_and_splits() {
        let s = consume(&[layer("old", 2, 100), layer("new", 10, 150)], 3, 999).unwrap();
        assert_eq!(
            s.iter().map(|x| (x.batch_item_id.as_str(), x.qty, x.unit_cost, x.revenue_net)).collect::<Vec<_>>(),
            vec![("old", 2, 100, 666), ("new", 1, 150, 333)]
        );
    }

    #[test]
    fn skips_empty_and_negative_layers() {
        let s = consume(&[layer("a", 0, 1), layer("b", -2, 2), layer("c", 5, 3)], 1, 10).unwrap();
        assert_eq!(s, vec![Slice { batch_item_id: "c".into(), qty: 1, unit_cost: 3, revenue_net: 10 }]);
    }

    #[test]
    fn oversell_draws_newest_negative() {
        let s = consume(&[layer("a", 1, 100), layer("b", 0, 200)], 3, 300).unwrap();
        assert_eq!(s.iter().map(|x| (x.batch_item_id.as_str(), x.qty)).collect::<Vec<_>>(), vec![("a", 1), ("b", 2)]);
        assert_eq!(consume(&[layer("a", 0, 100)], 2, 0).unwrap()[0].qty, 2);
        assert_eq!(
            consume(&[layer("a", 1, 100)], 4, 400).unwrap(),
            vec![Slice { batch_item_id: "a".into(), qty: 4, unit_cost: 100, revenue_net: 400 }]
        );
    }

    #[test]
    fn errors() {
        assert_eq!(consume(&[], 1, 0), Err(FifoError::NoLayers));
        assert_eq!(consume(&[layer("a", 1, 1)], 0, 0), Err(FifoError::NonPositiveQty));
        assert_eq!(consume(&[layer("a", 1, 1)], 1, -1), Err(FifoError::NegativeRevenue));
    }

    proptest! {
        #[test]
        fn slices_cover_line_exactly(
            on_hands in prop::collection::vec(-5i64..20, 1..6), qty in 1i64..60, revenue in 0i64..10_000_000,
        ) {
            let layers: Vec<Layer> = on_hands.iter().enumerate().map(|(i, h)| layer(&format!("L{i}"), *h, 100 + i as i64)).collect();
            let s = consume(&layers, qty, revenue).unwrap();
            prop_assert_eq!(s.iter().map(|x| x.qty).sum::<i64>(), qty);
            prop_assert_eq!(s.iter().map(|x| x.revenue_net).sum::<i64>(), revenue);
            prop_assert!(s.iter().all(|x| x.qty > 0 && x.revenue_net >= 0));
        }
    }
}
