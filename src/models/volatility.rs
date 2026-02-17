use crate::state::{VolRegime, VolatilityState};
use std::collections::VecDeque;

/// EWMA decay factor (lambda = 0.94 is standard for short-horizon)
const EWMA_LAMBDA: f64 = 0.94;

/// Threshold multiplier for jump detection (returns > JUMP_THRESHOLD * sigma)
const JUMP_THRESHOLD: f64 = 3.0;

/// Rolling window for jump intensity estimation (number of observations)
const JUMP_WINDOW: usize = 300;

/// Rolling window for regime detection (short vs long vol comparison)
const SHORT_VOL_WINDOW: usize = 30;
const LONG_VOL_WINDOW: usize = 300;

/// Regime threshold: short_vol / long_vol > this = high regime
const REGIME_THRESHOLD: f64 = 1.5;

/// Minimum samples before vol estimates are considered reliable
const MIN_SAMPLES: u64 = 20;

/// Volatility engine. Maintains state across ticks.
/// All updates are in-place, no allocations after construction.
pub struct VolatilityEngine {
    /// Recent log returns (ring buffer, pre-allocated)
    returns: VecDeque<f64>,
    /// Recent absolute returns for jump detection
    jump_buffer: VecDeque<f64>,
    /// Previous price for computing returns
    prev_price: f64,
    /// Current state (stack-allocated)
    pub state: VolatilityState,
}

impl VolatilityEngine {
    pub fn new() -> Self {
        Self {
            returns: VecDeque::with_capacity(LONG_VOL_WINDOW + 10),
            jump_buffer: VecDeque::with_capacity(JUMP_WINDOW + 10),
            prev_price: 0.0,
            state: VolatilityState::default(),
        }
    }

    /// Process a new BTC price observation. Updates all vol metrics in-place.
    /// Pure state transition: old_state + new_price -> new_state.
    #[inline]
    pub fn update(&mut self, price: f64) {
        if price <= 0.0 || !price.is_finite() {
            return;
        }

        if self.prev_price <= 0.0 {
            self.prev_price = price;
            return;
        }

        let log_return = (price / self.prev_price).ln();
        self.prev_price = price;

        if !log_return.is_finite() {
            return;
        }

        // Store return
        if self.returns.len() >= LONG_VOL_WINDOW {
            self.returns.pop_front();
        }
        self.returns.push_back(log_return);

        if self.jump_buffer.len() >= JUMP_WINDOW {
            self.jump_buffer.pop_front();
        }
        self.jump_buffer.push_back(log_return);

        self.state.sample_count += 1;

        // EWMA volatility update
        let r_sq = log_return * log_return;
        self.state.ewma_vol = (EWMA_LAMBDA * self.state.ewma_vol * self.state.ewma_vol
            + (1.0 - EWMA_LAMBDA) * r_sq)
            .sqrt();

        // Clamp vol to sane range
        self.state.ewma_vol = self.state.ewma_vol.clamp(1e-8, 1.0);

        if self.state.sample_count < MIN_SAMPLES {
            return;
        }

        // Jump detection
        self.update_jump_stats();

        // Regime detection
        self.update_regime();

        // Student-t degrees of freedom (method of moments)
        self.update_student_t_nu();
    }

    fn update_jump_stats(&mut self) {
        let sigma = self.state.ewma_vol;
        let threshold = JUMP_THRESHOLD * sigma;

        let mut jump_count: u32 = 0;
        let mut jump_sum: f64 = 0.0;
        let mut jump_sq_sum: f64 = 0.0;

        for &r in &self.jump_buffer {
            if r.abs() > threshold {
                jump_count += 1;
                jump_sum += r;
                jump_sq_sum += r * r;
            }
        }

        let n = self.jump_buffer.len() as f64;
        if n > 0.0 {
            // Poisson intensity: jumps per observation, annualized
            // Each observation ~2s apart, so ~43200 per day, ~15.7M per year
            let obs_per_year = 365.25 * 24.0 * 3600.0 / 2.0;
            self.state.jump_intensity = (jump_count as f64 / n) * obs_per_year;
        }

        if jump_count > 0 {
            let jc = jump_count as f64;
            self.state.jump_mean = jump_sum / jc;
            self.state.jump_var = if jump_count > 1 {
                (jump_sq_sum / jc) - (self.state.jump_mean * self.state.jump_mean)
            } else {
                sigma * sigma
            };
            self.state.jump_var = self.state.jump_var.max(1e-12);
        }
    }

    fn update_regime(&mut self) {
        if self.returns.len() < LONG_VOL_WINDOW {
            return;
        }

        let short_var = variance_of_last(&self.returns, SHORT_VOL_WINDOW);
        let long_var = variance_of_last(&self.returns, LONG_VOL_WINDOW);

        if long_var > 1e-16 {
            let ratio = short_var / long_var;
            self.state.regime = if ratio > REGIME_THRESHOLD {
                VolRegime::High
            } else {
                VolRegime::Low
            };
        }
    }

    fn update_student_t_nu(&mut self) {
        if self.returns.len() < 30 {
            return;
        }

        // Method of moments: kurtosis = 3 + 6/(nu-4) for nu > 4
        // => nu = 4 + 6/(kurtosis - 3)
        let n = self.returns.len() as f64;
        let mean = self.returns.iter().sum::<f64>() / n;
        let mut m2: f64 = 0.0;
        let mut m4: f64 = 0.0;
        for &r in &self.returns {
            let d = r - mean;
            let d2 = d * d;
            m2 += d2;
            m4 += d2 * d2;
        }
        m2 /= n;
        m4 /= n;

        if m2 > 1e-16 {
            let kurtosis = m4 / (m2 * m2);
            let excess = kurtosis - 3.0;

            if excess > 0.1 {
                let nu = 4.0 + 6.0 / excess;
                // Clamp to reasonable range
                self.state.student_t_nu = nu.clamp(2.5, 30.0);
            } else {
                // Near-Gaussian, use high nu
                self.state.student_t_nu = 30.0;
            }
        }
    }

    /// Get current volatility scaled to annual terms.
    /// For 15-min horizon: sigma_15min = ewma_vol * sqrt(observations_per_year)
    #[inline]
    pub fn annualized_vol(&self) -> f64 {
        // Each observation ~2s. Per year: 365.25 * 24 * 3600 / 2 = ~15_778_800
        // Annual vol = per-obs vol * sqrt(obs/year)
        let obs_per_year: f64 = 365.25 * 24.0 * 3600.0 / 2.0;
        self.state.ewma_vol * obs_per_year.sqrt()
    }

    #[inline]
    pub fn is_ready(&self) -> bool {
        self.state.sample_count >= MIN_SAMPLES
    }
}

/// Compute variance of the last `window` elements in a VecDeque. No allocation.
#[inline]
fn variance_of_last(data: &VecDeque<f64>, window: usize) -> f64 {
    let n = data.len().min(window);
    if n < 2 {
        return 0.0;
    }

    let start = data.len() - n;
    let nf = n as f64;

    let mut sum: f64 = 0.0;
    for i in start..data.len() {
        sum += data[i];
    }
    let mean = sum / nf;

    let mut var_sum: f64 = 0.0;
    for i in start..data.len() {
        let d = data[i] - mean;
        var_sum += d * d;
    }

    var_sum / (nf - 1.0)
}
