#![forbid(unsafe_code)]

//! Exact empirical ranks for bootstrap p-values, calibration grids and Holm decisions.

use num_rational::Ratio;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(into = "f64")]
pub struct PValue(Ratio<u32>);

impl PValue {
    pub const ZERO: Self = Self(Ratio::new_raw(0, 1));
    pub const ALPHA: Self = Self(Ratio::new_raw(1, 20));

    /// The observed statistic adds one to both the exceedance count and the reference size.
    pub fn empirical(exceedances: u16, replicas: u16) -> Self {
        Self(Ratio::new(
            u32::from(exceedances) + 1,
            u32::from(replicas) + 1,
        ))
    }

    pub fn round_up_to_grid(self, divisions: u16) -> Self {
        let grid = u32::from(divisions);
        let ticks = (self.0.numer() * grid).div_ceil(*self.0.denom());
        Self(Ratio::new(ticks, grid))
    }

    pub fn adjusted(self, multiplier: u16) -> Self {
        let denominator = *self.0.denom();
        Self(Ratio::new(
            (self.0.numer() * u32::from(multiplier)).min(denominator),
            denominator,
        ))
    }
}

impl From<PValue> for f64 {
    fn from(p: PValue) -> Self {
        Self::from(*p.0.numer()) / Self::from(*p.0.denom())
    }
}
