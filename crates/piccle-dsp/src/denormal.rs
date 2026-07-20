//! Portable subnormal flushing for recursive DSP state.

/// Flushes IEEE-754 subnormal values to signed zero.
///
/// This avoids data-dependent slow paths on processors that do not enable
/// hardware flush-to-zero. Normal values, including every value above the
/// specification's -180 dBFS floor, are unchanged.
#[inline]
pub(crate) const fn flush_subnormal(value: f64) -> f64 {
    const SIGN_MASK: u64 = 1_u64 << 63;
    const EXPONENT_MASK: u64 = 0x7ff0_0000_0000_0000;

    let bits = value.to_bits();
    if bits & EXPONENT_MASK == 0 { f64::from_bits(bits & SIGN_MASK) } else { value }
}

/// Flushes a value only in the specialized denormal-maintenance pass.
#[inline]
pub(crate) const fn flush_subnormal_if<const FLUSH: bool>(value: f64) -> f64 {
    if FLUSH { flush_subnormal(value) } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_the_sign_when_flushing() {
        assert_eq!(flush_subnormal(-f64::from_bits(1)).to_bits(), (-0.0_f64).to_bits());
    }

    #[test]
    fn leaves_minimum_normal_unchanged() {
        assert_eq!(flush_subnormal(f64::MIN_POSITIVE), f64::MIN_POSITIVE);
    }
}
