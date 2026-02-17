use crate::models::{PricingModel, VolContext};
use crate::state::ModelParams;
use statrs::distribution::{ContinuousCDF, Normal};

/// Black-Scholes digital option pricing.
///
/// P(S_T >= K) = Phi(d2)
///
/// where d2 = (ln(S/K) + (r - sigma^2/2)*T) / (sigma * sqrt(T))
/// and r = 0 for 15-minute horizon.
///
/// All computation uses precomputed ModelParams. No allocations.
pub struct BlackScholesDigital {
    /// Standard normal distribution (created once, reused)
    normal: Normal,
}

impl BlackScholesDigital {
    pub fn new() -> Self {
        // Normal::new(0, 1) only fails if std_dev <= 0; this is safe.
        let normal = Normal::new(0.0, 1.0)
            .unwrap_or_else(|_| {
                // This is structurally unreachable but we handle it gracefully
                tracing::error!("failed to create standard normal -- using fallback");
                Normal::new(0.0, 1.0).unwrap_or(Normal::standard())
            });
        Self { normal }
    }
}

impl PricingModel for BlackScholesDigital {
    #[inline]
    fn name(&self) -> &'static str {
        "Black-Scholes"
    }

    /// Pure function: probability from precomputed params.
    #[inline]
    fn probability(&self, params: &ModelParams, _vol_ctx: &VolContext) -> f64 {
        // Guard: if sigma or ttl is zero/negative, return 0.5 (no information)
        if params.sigma_sqrt_t < 1e-12 || params.ttl_years <= 0.0 {
            return if params.spot >= params.strike { 1.0 } else { 0.0 };
        }

        // d2 = (ln(S/K) - 0.5 * sigma^2 * T) / (sigma * sqrt(T))
        // Using precomputed values:
        let d2 = (params.ln_s_k - params.half_sigma_sq * params.ttl_years) / params.sigma_sqrt_t;

        let p = self.normal.cdf(d2);

        // Clamp to valid probability range
        p.clamp(0.001, 0.999)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atm_near_half() {
        let model = BlackScholesDigital::new();
        let params = ModelParams::new(100_000.0, 100_000.0, 900.0, 0.5);
        let ctx = VolContext { jump_intensity: 0.0, jump_mean: 0.0, jump_var: 0.0, student_t_nu: 5.0 };
        let p = model.probability(&params, &ctx);
        // ATM digital should be near 0.5 (slightly below due to drift term)
        assert!((p - 0.5).abs() < 0.1, "ATM prob={p} should be near 0.5");
    }

    #[test]
    fn test_deep_itm() {
        let model = BlackScholesDigital::new();
        let params = ModelParams::new(110_000.0, 100_000.0, 900.0, 0.5);
        let ctx = VolContext { jump_intensity: 0.0, jump_mean: 0.0, jump_var: 0.0, student_t_nu: 5.0 };
        let p = model.probability(&params, &ctx);
        assert!(p > 0.7, "deep ITM prob={p} should be > 0.7");
    }

    #[test]
    fn test_deep_otm() {
        let model = BlackScholesDigital::new();
        let params = ModelParams::new(90_000.0, 100_000.0, 900.0, 0.5);
        let ctx = VolContext { jump_intensity: 0.0, jump_mean: 0.0, jump_var: 0.0, student_t_nu: 5.0 };
        let p = model.probability(&params, &ctx);
        assert!(p < 0.3, "deep OTM prob={p} should be < 0.3");
    }
}
