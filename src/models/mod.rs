pub mod volatility;
pub mod black_scholes;
pub mod jump_diffusion;
pub mod student_t;
pub mod calibration;

use crate::state::ModelParams;

/// All pricing models implement this trait.
/// probability() must be a pure function: deterministic output from inputs only.
/// Send + Sync required for use across tokio tasks.
pub trait PricingModel: Send + Sync {
    fn name(&self) -> &'static str;

    /// Compute P(S_T >= K) given precomputed parameters and volatility context.
    /// Returns a probability in [0, 1]. Never panics.
    fn probability(&self, params: &ModelParams, vol_ctx: &VolContext) -> f64;
}

/// Additional volatility context passed to models that need it
/// (jump-diffusion needs lambda/delta, student-t needs nu).
/// Stack-allocated, Copy.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct VolContext {
    pub jump_intensity: f64,
    pub jump_mean: f64,
    pub jump_var: f64,
    pub student_t_nu: f64,
}
