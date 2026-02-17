use crate::models::{PricingModel, VolContext};
use crate::state::ModelParams;
use statrs::distribution::{ContinuousCDF, StudentsT};

/// Student-t distribution pricing for digital options.
///
/// Returns are modeled as: R ~ t_nu(0, sigma^2)
///
/// P(S_T >= K) = 1 - F_t( ln(K/S) / (sigma * sqrt(T)), nu )
///
/// This captures fat tails better than Gaussian for short BTC horizons.
pub struct StudentTDigital;

impl StudentTDigital {
    pub fn new() -> Self {
        Self
    }
}

impl PricingModel for StudentTDigital {
    #[inline]
    fn name(&self) -> &'static str {
        "Student-t"
    }

    fn probability(&self, params: &ModelParams, vol_ctx: &VolContext) -> f64 {
        if params.sigma_sqrt_t < 1e-12 || params.ttl_years <= 0.0 {
            return if params.spot >= params.strike { 1.0 } else { 0.0 };
        }

        let nu = vol_ctx.student_t_nu.clamp(2.1, 30.0);

        // Create Student-t distribution: location=0, scale=1, dof=nu
        let dist = match StudentsT::new(0.0, 1.0, nu) {
            Ok(d) => d,
            Err(_) => {
                // Fallback to normal approximation
                let normal = statrs::distribution::Normal::new(0.0, 1.0)
                    .unwrap_or(statrs::distribution::Normal::standard());
                let d2 = (params.ln_s_k - params.half_sigma_sq * params.ttl_years) / params.sigma_sqrt_t;
                return normal.cdf(d2).clamp(0.001, 0.999);
            }
        };

        // z = ln(K/S) / (sigma * sqrt(T))
        // Note: ln(K/S) = -ln(S/K) = -ln_s_k
        let z = -params.ln_s_k / params.sigma_sqrt_t;

        // P(S_T >= K) = P(R >= ln(K/S) / (sigma*sqrt(T))) = 1 - F_t(z)
        let p = 1.0 - dist.cdf(z);

        p.clamp(0.001, 0.999)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atm_near_half() {
        let model = StudentTDigital::new();
        let params = ModelParams::new(100_000.0, 100_000.0, 900.0, 0.5);
        let ctx = VolContext { jump_intensity: 0.0, jump_mean: 0.0, jump_var: 0.0, student_t_nu: 5.0 };
        let p = model.probability(&params, &ctx);
        assert!((p - 0.5).abs() < 0.05, "ATM Student-t prob={p} should be near 0.5");
    }

    #[test]
    fn test_fatter_tails_than_normal() {
        let st = StudentTDigital::new();
        let bs = crate::models::black_scholes::BlackScholesDigital::new();

        // Deep OTM: fat tails should assign higher probability
        let params = ModelParams::new(90_000.0, 100_000.0, 900.0, 0.5);
        let ctx = VolContext { jump_intensity: 0.0, jump_mean: 0.0, jump_var: 0.0, student_t_nu: 3.0 };

        let p_st = st.probability(&params, &ctx);
        let p_bs = bs.probability(&params, &ctx);

        assert!(p_st >= p_bs * 0.9, "Student-t ({p_st}) should have fatter tails than BS ({p_bs})");
    }
}
