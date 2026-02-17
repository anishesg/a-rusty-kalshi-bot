/// Robust Bayesian Kelly sizing.
///
/// Uses a Beta posterior for the win probability, applies conservative
/// shrinkage, fractional multiplier, and hard caps.
///
/// f_robust = gamma * (b * p_eff - (1 - p_eff)) / b
///
/// where:
///   p ~ Beta(alpha, beta)
///   p_eff = E[p] - lambda * sqrt(Var(p))
///   gamma = fractional Kelly multiplier
///   b = payout ratio = (1 - contract_price) / contract_price
///
/// All inputs/outputs are f64. Pure function.

/// Kelly sizing parameters. Stack-allocated.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct KellyParams {
    pub model_probability: f64, // Win probability from pricing model
    pub alpha: f64,            // Beta posterior alpha (wins + prior)
    pub beta: f64,             // Beta posterior beta (losses + prior)
    pub contract_price: f64,   // Current contract price (e.g. 0.55)
    pub fractional_gamma: f64, // Fractional Kelly multiplier [0.1, 0.3]
    pub lambda: f64,           // Conservative shrinkage factor (default 1.0 = 1-sigma)
    pub max_position: f64,     // Hard cap on number of contracts
}

/// Kelly sizing result. Stack-allocated.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct KellyResult {
    pub raw_fraction: f64,    // Full Kelly fraction
    pub robust_fraction: f64, // After shrinkage + fractional
    pub contracts: f64,       // Final position size in contracts
    pub p_eff: f64,           // Effective probability used
    pub p_mean: f64,          // Posterior mean
    pub p_std: f64,           // Posterior std dev
}

/// Compute robust Bayesian Kelly position size.
///
/// Uses the **model probability** as the base win probability, then applies
/// Bayesian uncertainty discount from the Beta posterior (trade history).
/// This ensures Kelly is responsive to model signals even with limited data,
/// while becoming tighter as we accumulate trade outcomes.
///
/// Pure function: deterministic from inputs.
#[inline]
pub fn compute_kelly(params: &KellyParams) -> KellyResult {
    let alpha = params.alpha.max(0.5);
    let beta_val = params.beta.max(0.5);
    let c = params.contract_price.clamp(0.01, 0.99);
    let model_p = params.model_probability.clamp(0.01, 0.99);

    // Beta distribution uncertainty (decreases as we get more trade history)
    let ab_sum = alpha + beta_val;
    let p_mean = alpha / ab_sum;
    let p_var = (alpha * beta_val) / (ab_sum * ab_sum * (ab_sum + 1.0));
    let p_std = p_var.sqrt();

    // Conservative estimate: start from MODEL probability, shrink by uncertainty
    // With no data (alpha=1,beta=1): p_std=0.289, large discount
    // After 100 trades: p_std≈0.035, small discount
    let p_eff = (model_p - params.lambda * p_std).clamp(0.01, 0.99);

    // Payout ratio for binary: b = (1-c)/c
    let b = (1.0 - c) / c;

    // Full Kelly: f* = (b*p - (1-p)) / b
    let raw_fraction = (b * p_eff - (1.0 - p_eff)) / b;

    // If raw Kelly is negative, no bet
    if raw_fraction <= 0.0 {
        return KellyResult {
            raw_fraction,
            robust_fraction: 0.0,
            contracts: 0.0,
            p_eff,
            p_mean,
            p_std,
        };
    }

    // Apply fractional multiplier
    let robust_fraction = raw_fraction * params.fractional_gamma;

    // Convert fraction to contracts (assuming $1 payoff per contract)
    // Position = fraction * bankroll / cost_per_contract
    // For paper trading, we just cap at max_position
    let contracts = (robust_fraction * params.max_position)
        .min(params.max_position)
        .max(0.0);

    KellyResult {
        raw_fraction,
        robust_fraction,
        contracts,
        p_eff,
        p_mean,
        p_std,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_edge_no_bet() {
        let params = KellyParams {
            model_probability: 0.5,
            alpha: 50.0,
            beta: 50.0,
            contract_price: 0.5,
            fractional_gamma: 0.2,
            lambda: 1.0,
            max_position: 50.0,
        };
        let result = compute_kelly(&params);
        // With model_prob=0.5 and conservative shrinkage, p_eff < 0.5
        // At contract_price=0.5, this should give no bet
        assert!(result.contracts < 1.0, "no edge should produce ~0 contracts: {}", result.contracts);
    }

    #[test]
    fn test_strong_edge_bets() {
        let params = KellyParams {
            model_probability: 0.8,
            alpha: 80.0,
            beta: 20.0,
            contract_price: 0.5,
            fractional_gamma: 0.2,
            lambda: 1.0,
            max_position: 50.0,
        };
        let result = compute_kelly(&params);
        assert!(result.contracts > 0.0, "strong edge should bet: {} contracts", result.contracts);
        assert!(result.contracts <= 50.0, "should not exceed max");
        assert!(result.robust_fraction < result.raw_fraction, "fractional should be less than full");
    }

    #[test]
    fn test_cap_respected() {
        let params = KellyParams {
            model_probability: 0.999,
            alpha: 999.0,
            beta: 1.0,
            contract_price: 0.1,
            fractional_gamma: 0.5,
            lambda: 0.0,
            max_position: 10.0,
        };
        let result = compute_kelly(&params);
        assert!(result.contracts <= 10.0, "cap must be respected: {}", result.contracts);
    }

    #[test]
    fn test_model_prob_with_no_history() {
        // Even with uninformative prior, model edge should produce a bet
        let params = KellyParams {
            model_probability: 0.6,
            alpha: 1.0,  // no trade history
            beta: 1.0,
            contract_price: 0.3,
            fractional_gamma: 0.2,
            lambda: 1.0,
            max_position: 50.0,
        };
        let result = compute_kelly(&params);
        // p_eff = 0.6 - 1.0*0.289 = 0.311 > break_even(0.3), should bet
        assert!(result.contracts > 0.0, "model edge with no history should bet: {} contracts", result.contracts);
    }
}
