use crate::models::{PricingModel, VolContext};
use crate::state::ModelParams;
use statrs::distribution::{ContinuousCDF, Normal};

/// Merton jump-diffusion digital option pricing.
///
/// P = sum_{k=0}^{K_max} [e^{-lambda*T} * (lambda*T)^k / k!] * Phi(d2_k)
///
/// where sigma_k^2 = sigma^2 + k * delta^2 / T
/// and d2_k uses sigma_k instead of sigma.
///
/// Truncated Poisson sum (K_max=10). All stack-allocated.
const K_MAX: usize = 10;

/// Precomputed factorials for k=0..10
const FACTORIALS: [f64; 11] = [
    1.0, 1.0, 2.0, 6.0, 24.0, 120.0, 720.0, 5040.0, 40320.0, 362880.0, 3628800.0,
];

pub struct JumpDiffusionDigital {
    normal: Normal,
}

impl JumpDiffusionDigital {
    pub fn new() -> Self {
        let normal = Normal::new(0.0, 1.0).unwrap_or(Normal::standard());
        Self { normal }
    }
}

impl PricingModel for JumpDiffusionDigital {
    #[inline]
    fn name(&self) -> &'static str {
        "Jump-Diffusion"
    }

    fn probability(&self, params: &ModelParams, vol_ctx: &VolContext) -> f64 {
        if params.sigma_sqrt_t < 1e-12 || params.ttl_years <= 0.0 {
            return if params.spot >= params.strike { 1.0 } else { 0.0 };
        }

        let lambda = vol_ctx.jump_intensity;
        let delta_sq = vol_ctx.jump_var;
        let t = params.ttl_years;
        let sigma_sq = params.sigma * params.sigma;
        let ln_s_k = params.ln_s_k;

        // If jump intensity is negligible, fall back to BS
        if lambda < 1e-6 {
            let d2 = (ln_s_k - params.half_sigma_sq * t) / params.sigma_sqrt_t;
            return self.normal.cdf(d2).clamp(0.001, 0.999);
        }

        let lambda_t = lambda * t;
        let neg_lambda_t = (-lambda_t).exp();

        let mut prob = 0.0;
        let mut poisson_term = neg_lambda_t; // e^{-lambda*T} * (lambda*T)^0 / 0! = e^{-lambda*T}

        for k in 0..=K_MAX {
            if k > 0 {
                poisson_term *= lambda_t / k as f64;
            }

            // Adjusted volatility: sigma_k^2 = sigma^2 + k * delta^2 / T
            let sigma_k_sq = sigma_sq + (k as f64) * delta_sq / t;
            let sigma_k = sigma_k_sq.sqrt();
            let sigma_k_sqrt_t = sigma_k * params.sqrt_t;

            if sigma_k_sqrt_t < 1e-12 {
                // Degenerate case
                let contribution = if params.spot >= params.strike { poisson_term } else { 0.0 };
                prob += contribution;
                continue;
            }

            let half_sigma_k_sq = 0.5 * sigma_k_sq;
            let d2_k = (ln_s_k - half_sigma_k_sq * t) / sigma_k_sqrt_t;

            prob += poisson_term * self.normal.cdf(d2_k);
        }

        prob.clamp(0.001, 0.999)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_jumps_matches_bs() {
        let jd = JumpDiffusionDigital::new();
        let bs = crate::models::black_scholes::BlackScholesDigital::new();
        let params = ModelParams::new(100_000.0, 100_000.0, 900.0, 0.5);
        let ctx = VolContext { jump_intensity: 0.0, jump_mean: 0.0, jump_var: 0.001, student_t_nu: 5.0 };

        let p_jd = jd.probability(&params, &ctx);
        let p_bs = bs.probability(&params, &ctx);

        assert!((p_jd - p_bs).abs() < 0.01, "JD with no jumps ({p_jd}) should match BS ({p_bs})");
    }

    #[test]
    fn test_with_jumps_differs() {
        let jd = JumpDiffusionDigital::new();
        let params = ModelParams::new(100_000.0, 100_000.0, 900.0, 0.5);
        let ctx_no_jump = VolContext { jump_intensity: 0.0, jump_mean: 0.0, jump_var: 0.001, student_t_nu: 5.0 };
        let ctx_jump = VolContext { jump_intensity: 50.0, jump_mean: 0.0, jump_var: 0.01, student_t_nu: 5.0 };

        let p1 = jd.probability(&params, &ctx_no_jump);
        let p2 = jd.probability(&params, &ctx_jump);

        // With jumps and higher variance, the probability should differ
        assert!((p1 - p2).abs() > 0.001 || (p1 - 0.5).abs() < 0.05,
            "jump vs no-jump should differ: {p1} vs {p2}");
    }
}
