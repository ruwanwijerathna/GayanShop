//! FIFO cost-layer consumption for sales (ARCHITECTURE §3.1).

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
    #[error("product has no cost layer; receive it through a batch first")]
    NoLayers,
}

/// Consume `qty` units from `layers` (ordered oldest → newest by batch receive time).
/// Oldest layers with positive stock go first; anything left over is drawn from the newest
/// layer, which goes negative (oversold — allowed offline, reported later).
/// `revenue_net` is split by qty, with the remainder on the last slice.
pub fn consume(layers: &[Layer], qty: i64, revenue_net: Money) -> Result<Vec<Slice>, FifoError> {
    if qty <= 0 {
        return Err(FifoError::NonPositiveQty);
    }
    let newest = layers.last().ok_or(FifoError::NoLayers)?;
    let mut remaining = qty;
    let mut picks: Vec<(String, i64, Money)> = Vec::new();
    for layer in layers {
        if remaining == 0 {
            break;
        }
        if layer.on_hand > 0 {
            let take = remaining.min(layer.on_hand);
            picks.push((layer.batch_item_id.clone(), take, layer.unit_cost));
            remaining -= take;
        }
    }
    if remaining > 0 {
        match picks.last_mut() {
            Some(last) if last.0 == newest.batch_item_id => last.1 += remaining,
            _ => picks.push((newest.batch_item_id.clone(), remaining, newest.unit_cost)),
        }
    }
    let mut slices = Vec::with_capacity(picks.len());
    let mut allocated = 0;
    let n = picks.len();
    for (i, (id, q, cost)) in picks.into_iter().enumerate() {
        let rev = if i + 1 == n { revenue_net - allocated } else { revenue_net * q / qty };
        allocated += rev;
        slices.push(Slice { batch_item_id: id, qty: q, unit_cost: cost, revenue_net: rev });
    }
    Ok(slices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(id: &str, on_hand: i64, cost: Money) -> Layer {
        Layer { batch_item_id: id.into(), on_hand, unit_cost: cost }
    }

    #[test]
    fn takes_oldest_first_and_splits() {
        let layers = [layer("old", 2, 100), layer("new", 10, 150)];
        let s = consume(&layers, 3, 999).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].batch_item_id.as_str(), s[0].qty, s[0].unit_cost), ("old", 2, 100));
        assert_eq!((s[1].batch_item_id.as_str(), s[1].qty, s[1].unit_cost), ("new", 1, 150));
        assert_eq!(s[0].revenue_net + s[1].revenue_net, 999);
        assert_eq!(s[0].revenue_net, 666);
    }

    #[test]
    fn skips_empty_and_negative_layers() {
        let layers = [layer("a", 0, 1), layer("b", -2, 2), layer("c", 5, 3)];
        let s = consume(&layers, 1, 10).unwrap();
        assert_eq!(s, vec![Slice { batch_item_id: "c".into(), qty: 1, unit_cost: 3, revenue_net: 10 }]);
    }

    #[test]
    fn oversell_draws_newest_negative() {
        let layers = [layer("a", 1, 100), layer("b", 0, 200)];
        let s = consume(&layers, 3, 300).unwrap();
        assert_eq!(s.iter().map(|x| (x.batch_item_id.as_str(), x.qty)).collect::<Vec<_>>(), vec![("a", 1), ("b", 2)]);
        let all_gone = [layer("a", 0, 100)];
        assert_eq!(consume(&all_gone, 2, 0).unwrap()[0].qty, 2);
    }

    #[test]
    fn oversell_merges_into_newest_slice() {
        let layers = [layer("a", 1, 100)];
        let s = consume(&layers, 4, 400).unwrap();
        assert_eq!(s, vec![Slice { batch_item_id: "a".into(), qty: 4, unit_cost: 100, revenue_net: 400 }]);
    }

    #[test]
    fn errors() {
        assert_eq!(consume(&[], 1, 0), Err(FifoError::NoLayers));
        assert_eq!(consume(&[layer("a", 1, 1)], 0, 0), Err(FifoError::NonPositiveQty));
    }
}
