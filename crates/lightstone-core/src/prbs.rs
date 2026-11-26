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

/// Length of the SSPRQ pattern in PAM-4 symbols.
pub const SSPRQ_LEN: usize = 65535;

/// PRBS31 sections of SSPRQ: (31-bit generator seed, number of bits). `eq:tx-ssprq`
pub const SSPRQ_SECTIONS: [(u32, usize); 3] = [
    (0x0000_0002, 10924),
    (0x3401_3FF7, 10922),
    (0x0CCC_CCCC, 10922),
];

/// `n` output bits of the IEEE 802.3 PRBS31 generator (polynomial 1 + x²⁸ + x³¹, Figure 49-9)
/// started from the 31-bit register value `seed` (bit 30 = S30). Each step shifts
/// S0 ← S27 ⊕ S30; the output is that bit inverted.
pub fn prbs31(n: usize, seed: u32) -> Vec<u8> {
    let mut s = seed & 0x7FFF_FFFF;
    assert!(s != 0, "PRBS31 seed must be nonzero");
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let b = ((s >> 30) ^ (s >> 27)) & 1;
        out.push((b ^ 1) as u8);
        s = ((s << 1) | b) & 0x7FFF_FFFF;
    }
    out
}

/// SSPRQ, the IEEE 802.3 short stress pattern for PAM-4 (65 535 symbols), as constructed in the
/// public P802.3bs contributions (anslow_01_0416_smf, phase-independent version, with the
/// generator change of anslow_02a_1016_logic). **Not verified against the published standard
/// (Clause 120.5.11.2.3, Table 120-2) or a scope's pattern file** – see docs/model/prbs-pattern.md.
///
/// 1. B = 32 768 bits: the three PRBS31 sections of [`SSPRQ_SECTIONS`] in sequence.
/// 2. D = B followed by B (65 536 bits).
/// 3. Part 1: D in bit pairs (first bit = MSB), Gray-coded: 32 768 symbols; symbols 16 384 … of
///    the second copy are inverted (3 − k).
/// 4. Part 2: D without its first and last bit (65 534 bits), paired and coded the same way:
///    32 767 symbols, symbols 16 384 … inverted. Part 2 uses the other bit-pair phase.
/// 5. SSPRQ = part 1 followed by part 2.
pub fn ssprq() -> Vec<u8> {
    let mut b = Vec::with_capacity(32768);
    for (seed, n) in SSPRQ_SECTIONS {
        b.extend(prbs31(n, seed));
    }
    let d: Vec<u8> = b.iter().chain(b.iter()).copied().collect();
    let half = b.len() / 2;
    let code = |bits: &[u8]| -> Vec<u8> {
        bits.as_chunks::<2>()
            .0
            .iter()
            .enumerate()
            .map(|(j, p)| {
                let k = gray_pam4(p[0], p[1]);
                if j >= half {
                    3 - k
                } else {
                    k
                }
            })
            .collect()
    };
    let mut out = code(&d);
    out.extend(code(&d[1..d.len() - 1]));
    debug_assert_eq!(out.len(), SSPRQ_LEN);
    out
}

/// Named PAM-4 test pattern: `"prbs13q"` (8191 symbols, `seed` = PRBS13 seed) or `"ssprq"`
/// (65 535 symbols, fixed). NRZ (`levels` = 2) supports only PRBS13.
pub fn named_pattern(name: &str, levels: u8, seed: u16) -> Result<Vec<u8>, String> {
    match (name, levels) {
        ("prbs13q" | "prbs", _) => pattern(levels, seed),
        ("ssprq", 4) => Ok(ssprq()),
        ("ssprq", _) => Err("SSPRQ is a PAM-4 pattern (levels = 4)".into()),
        (o, _) => Err(format!("unknown pattern '{o}' (prbs13q or ssprq)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prbs31_recurrence_and_inverted_output() {
        // Raw bits b satisfy b[n] = b[n-28] ⊕ b[n-31]; the output is ¬b.
        let o = prbs31(500, 0x1234_5678);
        for n in 31..o.len() {
            assert_eq!(o[n] ^ 1, (o[n - 28] ^ 1) ^ (o[n - 31] ^ 1), "bit {n}");
        }
    }

    #[test]
    fn ssprq_structure() {
        let s = ssprq();
        assert_eq!(s.len(), SSPRQ_LEN);
        assert!(s.iter().all(|&k| k < 4));
        // Part 1: the second 16 384 symbols are the inverted first ones.
        for j in 0..16384 {
            assert_eq!(s[16384 + j], 3 - s[j]);
        }
        // Every level occurs, and the inversion balances the outer and inner levels.
        let mut c = [0usize; 4];
        for &k in &s[..32768] {
            c[k as usize] += 1;
        }
        assert_eq!(c[0], c[3]);
        assert_eq!(c[1], c[2]);
        assert_eq!(named_pattern("ssprq", 4, 1).unwrap(), s);
        assert!(named_pattern("ssprq", 2, 1).is_err());
        assert!(named_pattern("prbs31q", 4, 1).is_err());
    }

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
