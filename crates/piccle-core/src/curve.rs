//! Transition curve primitives.
//!
//! Spec: piccle-spec/docs/10-curves.md. All curves map progress `t` in
//! `[0, 1]` to an eased progress value; `exponential` is value-relative and
//! therefore depends on the segment endpoints.

/// Floor applied to exponential curve endpoints to keep them positive.
/// Spec: piccle-spec/docs/10-curves.md (exponential endpoint clamp).
pub const EXPONENTIAL_ENDPOINT_FLOOR: f64 = 1e-10;

/// Transition curve shapes used in volume, pitch, and filter entries.
///
/// Spec: piccle-spec/schemas/v1.json `$defs/curves`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Curve {
    /// Constant-rate interpolation.
    Linear,
    /// Geometric interpolation (value-relative).
    Exponential,
    /// Quadratic acceleration (`t^2`).
    EaseIn,
    /// Quadratic deceleration (`1 - (1-t)^2`).
    EaseOut,
    /// Symmetric rational ease (`t^2 / (t^2 + (1-t)^2)`).
    EaseInOut,
}

impl Curve {
    /// Parses the schema enum spelling.
    #[must_use]
    pub fn from_schema_name(name: &str) -> Option<Self> {
        match name {
            "linear" => Some(Self::Linear),
            "exponential" => Some(Self::Exponential),
            "easeIn" => Some(Self::EaseIn),
            "easeOut" => Some(Self::EaseOut),
            "easeInOut" => Some(Self::EaseInOut),
            _ => None,
        }
    }

    /// Schema enum spelling of this curve.
    #[must_use]
    pub const fn schema_name(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Exponential => "exponential",
            Self::EaseIn => "easeIn",
            Self::EaseOut => "easeOut",
            Self::EaseInOut => "easeInOut",
        }
    }

    /// Interpolated value at progress `t` between `start` and `target`.
    ///
    /// Spec: piccle-spec/docs/10-curves.md.
    ///
    /// - linear: `start + (target - start) * t`
    /// - exponential: `s * (e / s)^t` with `s = max(start, 1e-10)`, `e =
    ///   max(target, 1e-10)`
    /// - ease*: `start + (target - start) * p(t)`
    #[must_use]
    pub fn value(self, start: f64, target: f64, t: f64) -> f64 {
        let progress = match self {
            Self::Linear => t,
            Self::Exponential => {
                let s = start.max(EXPONENTIAL_ENDPOINT_FLOOR);
                let e = target.max(EXPONENTIAL_ENDPOINT_FLOOR);
                return s * (e / s).powf(t);
            }
            Self::EaseIn => t * t,
            Self::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Self::EaseInOut => {
                let numerator = t * t;
                let denominator = numerator + (1.0 - t) * (1.0 - t);
                numerator / denominator
            }
        };
        start + (target - start) * progress
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_name_round_trips_every_curve() {
        for curve in
            [Curve::Linear, Curve::Exponential, Curve::EaseIn, Curve::EaseOut, Curve::EaseInOut]
        {
            assert_eq!(Curve::from_schema_name(curve.schema_name()), Some(curve));
        }
    }

    #[test]
    fn from_schema_name_rejects_unknown_names() {
        assert_eq!(Curve::from_schema_name("step"), None);
    }
}
