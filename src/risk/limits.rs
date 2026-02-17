use crate::state::{ModelState, VolRegime, VolatilityState};

/// Risk limit check result
#[derive(Debug, Clone, Copy)]
pub enum RiskCheck {
    /// Trading allowed
    Allowed,
    /// Blocked with reason
    Blocked(&'static str),
}

impl RiskCheck {
    #[inline]
    pub fn is_allowed(&self) -> bool {
        matches!(self, RiskCheck::Allowed)
    }
}

/// Check all risk limits before placing a trade.
/// Pure function, no side effects.
#[inline]
pub fn check_risk_limits(
    model: &ModelState,
    vol_state: &VolatilityState,
    proposed_contracts: f64,
    proposed_price: f64,
    max_daily_drawdown: f64,
    max_position: f64,
) -> RiskCheck {
    // 1. Daily drawdown stop
    if model.daily_pnl < -max_daily_drawdown {
        return RiskCheck::Blocked("daily drawdown limit breached");
    }

    // 2. Max position size
    let new_exposure = model.current_exposure + proposed_contracts * proposed_price;
    if new_exposure > max_position {
        return RiskCheck::Blocked("max position size exceeded");
    }

    // 3. Volatility spike stop: refuse trade in high-vol regime if drawdown is elevated
    if vol_state.regime == VolRegime::High && model.max_drawdown > max_daily_drawdown * 0.5 {
        return RiskCheck::Blocked("vol spike + elevated drawdown");
    }

    // 4. Minimum trade size
    if proposed_contracts < 0.01 {
        return RiskCheck::Blocked("trade size too small");
    }

    // 5. Sanity: contract price must be valid
    if proposed_price <= 0.0 || proposed_price >= 1.0 {
        return RiskCheck::Blocked("invalid contract price");
    }

    RiskCheck::Allowed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_conditions_allowed() {
        let model = ModelState::new("test");
        let vol = VolatilityState::default();
        let check = check_risk_limits(&model, &vol, 10.0, 0.5, 100.0, 50.0);
        assert!(check.is_allowed());
    }

    #[test]
    fn test_drawdown_blocks() {
        let mut model = ModelState::new("test");
        model.daily_pnl = -150.0;
        let vol = VolatilityState::default();
        let check = check_risk_limits(&model, &vol, 10.0, 0.5, 100.0, 50.0);
        assert!(!check.is_allowed());
    }
}
