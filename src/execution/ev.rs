/// Execution-adjusted expected value computation.
///
/// EV = q * [p * (1 - f) - c - s]
///
/// where:
///   p = calibrated model probability
///   c = contract cost (price to buy yes contract)
///   f = fee rate
///   s = slippage estimate
///   q = fill probability
///
/// All inputs are f64. Pure function, no side effects, no allocations.

/// Parameters for EV computation. Stack-allocated.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct EvParams {
    pub probability: f64,     // Model probability P(S_T >= K)
    pub contract_price: f64,  // Cost of YES contract (e.g. 0.55 = 55 cents)
    pub fee_rate: f64,        // Fee as fraction of payout (e.g. 0.02)
    pub slippage: f64,        // Estimated slippage in dollars (e.g. 0.005)
    pub fill_probability: f64, // Probability of getting filled (e.g. 0.9)
}

/// Result of EV computation. Stack-allocated.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct EvResult {
    /// Raw expected value per contract
    pub ev: f64,
    /// Whether this exceeds the threshold
    pub is_signal: bool,
    /// Recommended side: true = buy YES, false = buy NO
    pub buy_yes: bool,
    /// The effective probability used
    pub effective_prob: f64,
    /// EV of the opposite side (for comparison)
    pub ev_opposite: f64,
}

/// Compute execution-adjusted EV for both YES and NO sides.
/// Returns the better side if either has positive EV above threshold.
///
/// This is a **pure function**: same inputs always produce same output.
#[inline]
pub fn compute_ev(params: &EvParams, threshold: f64) -> EvResult {
    let p = params.probability;
    let c = params.contract_price;
    let f = params.fee_rate;
    let s = params.slippage;
    let q = params.fill_probability;

    // EV of buying YES at price c:
    // Win: p * (1.0 - c) * (1 - f)  (payout is $1, paid c, net gain is (1-c), minus fees)
    // Lose: (1-p) * (-c)
    // Total EV = p * (1-c) * (1-f) - (1-p) * c - s
    let ev_yes = q * (p * (1.0 - c) * (1.0 - f) - (1.0 - p) * c - s);

    // EV of buying NO at price (1-c):
    // This is equivalent to selling YES / buying NO
    // Win: (1-p) * c * (1-f) - p * (1-c) - s
    let no_price = 1.0 - c;
    let ev_no = q * ((1.0 - p) * (1.0 - no_price) * (1.0 - f) - p * no_price - s);

    if ev_yes >= ev_no && ev_yes > threshold {
        EvResult {
            ev: ev_yes,
            is_signal: true,
            buy_yes: true,
            effective_prob: p,
            ev_opposite: ev_no,
        }
    } else if ev_no > ev_yes && ev_no > threshold {
        EvResult {
            ev: ev_no,
            is_signal: true,
            buy_yes: false,
            effective_prob: 1.0 - p,
            ev_opposite: ev_yes,
        }
    } else {
        EvResult {
            ev: ev_yes.max(ev_no),
            is_signal: false,
            buy_yes: ev_yes >= ev_no,
            effective_prob: p,
            ev_opposite: ev_yes.min(ev_no),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fair_price_zero_ev() {
        let params = EvParams {
            probability: 0.5,
            contract_price: 0.5,
            fee_rate: 0.0,
            slippage: 0.0,
            fill_probability: 1.0,
        };
        let result = compute_ev(&params, 0.01);
        assert!(!result.is_signal, "fair price should not signal");
        assert!(result.ev.abs() < 0.01, "fair price EV should be ~0: {}", result.ev);
    }

    #[test]
    fn test_edge_signals() {
        let params = EvParams {
            probability: 0.7,
            contract_price: 0.5,
            fee_rate: 0.01,
            slippage: 0.005,
            fill_probability: 0.95,
        };
        let result = compute_ev(&params, 0.02);
        assert!(result.is_signal, "should signal when model has edge");
        assert!(result.buy_yes, "should buy YES when prob > price");
        assert!(result.ev > 0.0, "EV should be positive");
    }

    #[test]
    fn test_no_side_edge() {
        let params = EvParams {
            probability: 0.3,
            contract_price: 0.5,
            fee_rate: 0.01,
            slippage: 0.005,
            fill_probability: 0.95,
        };
        let result = compute_ev(&params, 0.02);
        if result.is_signal {
            assert!(!result.buy_yes, "should buy NO when prob < price");
        }
    }
}
