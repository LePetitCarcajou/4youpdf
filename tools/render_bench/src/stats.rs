//! Order statistics over measurements.

/// The values in increasing order, NaN left out.
fn sorted(values: &[f64]) -> Vec<f64> {
    let mut sorted: Vec<f64> = values.iter().copied().filter(|x| !x.is_nan()).collect();
    sorted.sort_by(f64::total_cmp);
    sorted
}

/// The middle value, or the mean of the two middle ones; `None` without
/// values.
pub fn median(values: &[f64]) -> Option<f64> {
    let sorted = sorted(values);
    let half = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted.get(half).copied()
    } else {
        Some((sorted.get(half.checked_sub(1)?)? + sorted.get(half)?) / 2.0)
    }
}

/// Nearest-rank percentile: the smallest value that at least `p` percent of
/// the values do not exceed. `None` without values.
pub fn percentile(values: &[f64], p: f64) -> Option<f64> {
    let sorted = sorted(values);
    if sorted.is_empty() {
        return None;
    }
    let rank = (p.clamp(0.0, 100.0) / 100.0 * sorted.len() as f64).ceil() as usize;
    sorted.get(rank.clamp(1, sorted.len()) - 1).copied()
}

/// Arithmetic mean; `None` without values.
pub fn mean(values: &[f64]) -> Option<f64> {
    let sorted = sorted(values);
    (!sorted.is_empty()).then(|| sorted.iter().sum::<f64>() / sorted.len() as f64)
}

/// Smallest value; `None` without values.
pub fn min(values: &[f64]) -> Option<f64> {
    sorted(values).first().copied()
}

/// Largest value; `None` without values.
pub fn max(values: &[f64]) -> Option<f64> {
    sorted(values).last().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_statistics() {
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[3.0]), Some(3.0));
        assert_eq!(median(&[4.0, 1.0, 3.0]), Some(3.0));
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), Some(2.5));
        assert_eq!(median(&[f64::NAN, 1.0]), Some(1.0));
        let ten: Vec<f64> = (1..=10).map(f64::from).collect();
        assert_eq!(percentile(&ten, 90.0), Some(9.0));
        assert_eq!(percentile(&ten, 100.0), Some(10.0));
        assert_eq!(percentile(&ten, 0.0), Some(1.0));
        assert_eq!(percentile(&[], 50.0), None);
        assert_eq!(mean(&[1.0, 2.0, 6.0]), Some(3.0));
        assert_eq!(min(&[2.0, -1.0]), Some(-1.0));
        assert_eq!(max(&[2.0, -1.0]), Some(2.0));
    }
}
