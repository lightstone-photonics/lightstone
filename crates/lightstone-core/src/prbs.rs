//! Test patterns: PRBS13 (NRZ) and PRBS13Q (Gray-coded PAM-4).
//!
//! PRBS13 uses the IEEE 802.3 polynomial 1 + x + x² + x¹² + x¹³ (maximal length, period 8191).
//! PRBS13Q is formed from two PRBS13 periods (16382 bits) taken in pairs (first bit = MSB)
//! and Gray-coded to 8191 PAM-4 symbols. For NRZ, one PRBS13 period (8191 bits) is used
//! directly, so both patterns have the same period. (802.3 NRZ PMDs specify PRBS31 or SSPR
//! for some tests; PRBS13 is a modelling choice that keeps the simulation grid unchanged.)
//!
//! Note: bit-exact conformance (seed and bit order) to IEEE 802.3 Clause 120.5.11.2.1 has not
//! yet been verified against the standard's reference sequence.

/// Default nonzero LFSR seed.
pub const PRBS13_DEFAULT_SEED: u16 = 0x0AAB;

/// Generate `n` bits of PRBS13 from a 13-bit nonzero seed.
pub fn prbs13(n: usize, seed: u16) -> Vec<u8> {
    let mut s: u16 = seed & 0x1FFF;
    assert!(s != 0, "PRBS13 seed must be nonzero");
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        // Taps at x^13, x^12, x^2, x^1 (bit indices 12, 11, 1, 0).
        let b = ((s >> 12) ^ (s >> 11) ^ (s >> 1) ^ s) & 1;
        out.push((s >> 12) as u8 & 1);
        s = ((s << 1) | b) & 0x1FFF;
    }
    out
}

/// Gray-code a bit pair (MSB, LSB) to a PAM-4 level index 0..=3.
/// `eq:tx-gray`: 00→0, 01→1, 11→2, 10→3
pub fn gray_pam4(msb: u8, lsb: u8) -> u8 {
    match (msb & 1, lsb & 1) {
        (0, 0) => 0,
        (0, 1) => 1,
        (1, 1) => 2,
        _ => 3,
    }
}

/// One period of PRBS13Q: 8191 PAM-4 symbols in {0, 1, 2, 3}.
pub fn prbs13q(seed: u16) -> Vec<u8> {
    let bits = prbs13(2 * 8191, seed);
    bits.as_chunks::<2>()
        .0
        .iter()
        .map(|p| gray_pam4(p[0], p[1]))
        .collect()
}

/// One period of the test pattern for an alphabet of `levels` symbols: PRBS13Q (8191 PAM-4
/// symbols) for 4, PRBS13 (8191 NRZ bits) for 2. Both have the period
/// [`crate::signal::PATTERN_PERIOD`].
pub fn pattern(levels: u8, seed: u16) -> Result<Vec<u8>, String> {
    match levels {
        4 => Ok(prbs13q(seed)),
        2 => Ok(prbs13(8191, seed)),
        _ => Err(format!("levels must be 2 (NRZ) or 4 (PAM-4), got {levels}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nrz_pattern_is_one_prbs13_period() {
        let p = pattern(2, PRBS13_DEFAULT_SEED).unwrap();
        assert_eq!(p, prbs13(8191, PRBS13_DEFAULT_SEED));
        assert_eq!(pattern(4, 7).unwrap(), prbs13q(7));
        assert!(pattern(3, 7).is_err());
    }

    #[test]
    fn prbs13_maximal_length() {
        // State sequence must have period exactly 8191: the 13-bit windows of the output
        // must all be distinct over one period.
        let bits = prbs13(8191 + 13, 1);
        let mut seen = vec![false; 8192];
        for i in 0..8191 {
            let mut w = 0usize;
            for j in 0..13 {
                w = (w << 1) | bits[i + j] as usize;
            }
            assert!(!seen[w], "window repeated at {i}");
            seen[w] = true;
        }
        assert!(!seen[0], "all-zero window must not occur");
        // Periodicity.
        let long = prbs13(3 * 8191, 1);
        assert_eq!(&long[..8191], &long[8191..2 * 8191]);
    }

    #[test]
    fn prbs13_balance() {
        let bits = prbs13(8191, PRBS13_DEFAULT_SEED);
        let ones: usize = bits.iter().map(|&b| b as usize).sum();
        assert_eq!(ones, 4096);
    }

    #[test]
    fn prbs13q_symbol_counts() {
        let s = prbs13q(PRBS13_DEFAULT_SEED);
        assert_eq!(s.len(), 8191);
        let mut counts = [0usize; 4];
        for &v in &s {
            counts[v as usize] += 1;
        }
        // Two PRBS13 periods contain every nonzero 13-bit state twice; level counts are
        // balanced to within one symbol.
        for c in counts {
            assert!((2046..=2049).contains(&c), "{counts:?}");
        }
    }
}
