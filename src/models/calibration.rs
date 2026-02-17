/// Isotonic regression calibrator.
///
/// Buckets model predictions into bins, tracks realized frequency,
/// and applies pool-adjacent-violators (PAV) to produce calibrated probabilities.
///
/// All operations are in-place on fixed-size arrays. No heap allocation after init.

const NUM_BUCKETS: usize = 10;

#[derive(Debug, Clone)]
pub struct Calibrator {
    /// Per-bucket: (predicted_count, realized_count)
    buckets: [(u64, u64); NUM_BUCKETS],
    /// Calibrated probabilities per bucket (output of PAV)
    calibrated: [f64; NUM_BUCKETS],
    /// Total observations
    total: u64,
}

impl Calibrator {
    pub fn new() -> Self {
        Self {
            buckets: [(0, 0); NUM_BUCKETS],
            calibrated: [0.05, 0.15, 0.25, 0.35, 0.45, 0.55, 0.65, 0.75, 0.85, 0.95],
            total: 0,
        }
    }

    /// Record an observation: model predicted `prob`, actual outcome was `realized` (0 or 1).
    pub fn record(&mut self, prob: f64, realized: bool) {
        let bucket = prob_to_bucket(prob);
        self.buckets[bucket].0 += 1;
        if realized {
            self.buckets[bucket].1 += 1;
        }
        self.total += 1;

        // Re-run PAV every 20 observations
        if self.total % 20 == 0 {
            self.run_pav();
        }
    }

    /// Apply calibration to a raw model probability.
    #[inline]
    pub fn calibrate(&self, prob: f64) -> f64 {
        if self.total < 50 {
            // Not enough data for calibration, pass through
            return prob;
        }
        let bucket = prob_to_bucket(prob);
        self.calibrated[bucket]
    }

    /// Mean absolute calibration error across buckets with data.
    pub fn calibration_error(&self) -> f64 {
        let mut err_sum = 0.0;
        let mut count = 0;
        for (i, &(pred_n, real_n)) in self.buckets.iter().enumerate() {
            if pred_n > 0 {
                let expected = (i as f64 + 0.5) / NUM_BUCKETS as f64;
                let actual = real_n as f64 / pred_n as f64;
                err_sum += (expected - actual).abs();
                count += 1;
            }
        }
        if count == 0 { 0.0 } else { err_sum / count as f64 }
    }

    /// Pool Adjacent Violators algorithm for isotonic regression.
    /// Ensures calibrated[i] <= calibrated[i+1].
    fn run_pav(&mut self) {
        // Compute raw frequencies
        let mut values: [f64; NUM_BUCKETS] = [0.0; NUM_BUCKETS];
        let mut weights: [f64; NUM_BUCKETS] = [0.0; NUM_BUCKETS];

        for i in 0..NUM_BUCKETS {
            let (n, r) = self.buckets[i];
            if n > 0 {
                values[i] = r as f64 / n as f64;
                weights[i] = n as f64;
            } else {
                // Use midpoint as default
                values[i] = (i as f64 + 0.5) / NUM_BUCKETS as f64;
                weights[i] = 0.1; // Small pseudo-weight
            }
        }

        // PAV: merge adjacent violators
        // Use stack-allocated arrays (no Vec)
        let mut pooled_val: [f64; NUM_BUCKETS] = values;
        let mut pooled_wt: [f64; NUM_BUCKETS] = weights;
        let mut pooled_len: usize = NUM_BUCKETS;
        let mut pool_start: [usize; NUM_BUCKETS] = [0; NUM_BUCKETS];
        let mut pool_end: [usize; NUM_BUCKETS] = [0; NUM_BUCKETS];

        for i in 0..NUM_BUCKETS {
            pool_start[i] = i;
            pool_end[i] = i;
        }

        let mut changed = true;
        let mut iterations = 0;
        while changed && iterations < 100 {
            changed = false;
            iterations += 1;

            let mut i = 0;
            while i + 1 < pooled_len {
                if pooled_val[i] > pooled_val[i + 1] {
                    // Merge pools i and i+1
                    let new_wt = pooled_wt[i] + pooled_wt[i + 1];
                    let new_val = (pooled_val[i] * pooled_wt[i] + pooled_val[i + 1] * pooled_wt[i + 1]) / new_wt;

                    pooled_val[i] = new_val;
                    pooled_wt[i] = new_wt;
                    pool_end[i] = pool_end[i + 1];

                    // Shift remaining pools left
                    for j in (i + 1)..(pooled_len - 1) {
                        pooled_val[j] = pooled_val[j + 1];
                        pooled_wt[j] = pooled_wt[j + 1];
                        pool_start[j] = pool_start[j + 1];
                        pool_end[j] = pool_end[j + 1];
                    }
                    pooled_len -= 1;
                    changed = true;
                } else {
                    i += 1;
                }
            }
        }

        // Write back calibrated values
        for p in 0..pooled_len {
            let val = pooled_val[p].clamp(0.001, 0.999);
            for b in pool_start[p]..=pool_end[p] {
                if b < NUM_BUCKETS {
                    self.calibrated[b] = val;
                }
            }
        }
    }
}

#[inline]
fn prob_to_bucket(prob: f64) -> usize {
    let idx = (prob * NUM_BUCKETS as f64) as usize;
    idx.min(NUM_BUCKETS - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bucket_mapping() {
        assert_eq!(prob_to_bucket(0.0), 0);
        assert_eq!(prob_to_bucket(0.05), 0);
        assert_eq!(prob_to_bucket(0.15), 1);
        assert_eq!(prob_to_bucket(0.95), 9);
        assert_eq!(prob_to_bucket(1.0), 9);
    }

    #[test]
    fn test_calibration_passthrough_with_few_samples() {
        let cal = Calibrator::new();
        let p = cal.calibrate(0.7);
        assert!((p - 0.7).abs() < 1e-10, "should pass through with few samples");
    }

    #[test]
    fn test_pav_monotonicity() {
        let mut cal = Calibrator::new();
        // Feed data that should produce monotonic calibration
        for _ in 0..100 {
            cal.record(0.2, false);
            cal.record(0.8, true);
        }
        for i in 0..9 {
            assert!(cal.calibrated[i] <= cal.calibrated[i + 1],
                "PAV should be monotonic: bucket {i}={} > bucket {}={}",
                cal.calibrated[i], i + 1, cal.calibrated[i + 1]);
        }
    }
}
