---
component: prbs-pattern
title: Pattern source (PRBS13Q / SSPRQ / PRBS13)
validation: [V15]
references: [ieee-802.3, anslow-ssprq-2016, anslow-sspr-generation-2016, dawe-ssprq-2017]
---

## What it models

The test data of the link: a pseudo-random symbol sequence, as used to specify and measure optical PAM-4 and NRZ links. Every short combination of symbols occurs (almost) equally often, so the pattern exercises all transitions the link has to carry. Node kind `prbs13q`; its output is a data signal (symbols and alphabet size), with no noise. The driver turns it into a drive waveform (`driver.md`); the analyzers, the FFE and TDECQ use it as the reference sequence (`ref` inputs).

## Equations

**PRBS13** (`eq:tx-prbs13`): a 13-bit linear-feedback shift register with polynomial $1 + x + x^2 + x^{12} + x^{13}$. Here $x^k$ stands for a delay of $k$ bits: each new bit is the XOR (sum modulo 2) of the bits 1, 2, 12 and 13 positions earlier. The register starts from a nonzero 13-bit seed and runs through all $2^{13} - 1 = 8191$ nonzero states, so the bit sequence repeats after 8191 bits (maximal length).

**PRBS13Q** (`eq:tx-gray`), for PAM-4 (`levels: 4`, default): two PRBS13 periods (16 382 bits) are taken in pairs, the first bit as MSB, and Gray-coded to a symbol $k \in \lbrace0, 1, 2, 3\rbrace$:

$$00 \to 0,\qquad 01 \to 1,\qquad 11 \to 2,\qquad 10 \to 3$$

Neighbouring symbols differ in one bit, so a symbol error to the next level costs one bit error (BER ≈ SER/2).

**NRZ** (`levels: 2`): one PRBS13 period of 8191 bits is used directly as symbols $k \in \lbrace0, 1\rbrace$.

**SSPRQ** (`eq:tx-ssprq`, `pattern: "ssprq"`, PAM-4 only): the short stress pattern of IEEE 802.3 for PAM-4 transmitter tests such as TDECQ, 65 535 symbols per period. It contains long runs and worst-case transitions that PRBS13Q lacks, so it stresses baseline wander and clock recovery more than random data does. Construction, as implemented:

1. The bit block $B$ (32 768 bits) consists of three sections of the PRBS31 sequence (polynomial $1 + x^{28} + x^{31}$, the generator of IEEE 802.3 Figure 49-9 with its inverted output), started from the 31-bit register values `0x00000002`, `0x34013FF7` and `0x0CCCCCCC`, of 10 924, 10 922 and 10 922 bits.
2. $D$ is $B$ followed by $B$ (65 536 bits).
3. Part 1: $D$ in bit pairs (first bit as MSB), Gray-coded as above, gives 32 768 symbols. The symbols of the second copy (from symbol 16 384 on) are inverted, $k \to 3 - k$.
4. Part 2: $D$ without its first and last bit (65 534 bits), paired and coded the same way, gives 32 767 symbols, again inverted from symbol 16 384 on. Dropping one bit uses the other bit-pair phase, so the pattern is stressful for either Gray-coding phase.
5. SSPRQ is part 1 followed by part 2: $32\thinspace768 + 32\thinspace767 = 65\thinspace535$ symbols.

SSPRQ needs a longer simulation grid: the graph setting `pattern_length` must be 65 535 (the editor sets it when you pick SSPRQ). A mismatch stops the run with a message. The grid has 8 times as many samples as with PRBS13Q, so runs take correspondingly longer.

PRBS13Q and PRBS13 have $N_\text{sym} = 8191$ symbols per period (`pattern_length` 8191, the default), so the simulation grid is the same for both. With `pattern_periods` $= p$ the source repeats the pattern $p$ times, so $N_\text{sym} = p$ × `pattern_length`; the record is one period of an endlessly repeating signal (signal-spec 1).

## Assumptions & limits

- **SSPRQ is not yet verified against the standard.** The construction above follows the public P802.3bs contributions: the pattern of anslow_01_0416_smf (phase-independent version) with the generator change of anslow_02a_1016_logic (the hex values are the generator's seed, and its output is inverted). The published text (IEEE 802.3 Clause 120.5.11.2.3, Table 120-2) has not been compared, and three details could differ: the first seed (dawe_3bs_01a_0317 proposed `0x022A0ED0`; whether the standard adopted it is not known here), how a seed is loaded into the register (here: bit 30 = S30), and which part-2 symbol is the first inverted one (here: 16 384). Any of these changes the symbol sequence completely, so a measured SSPRQ waveform only matches if the construction is exact. For measured data, give the transmitted symbols from the scope or pattern generator's file (`measured-data.md`); the editor labels the pattern *unverified*.
- The seed and bit order of PRBS13Q have not been checked bit for bit against IEEE 802.3 Clause 120.5.11.2.1. Results depend on the pattern only through its statistics, so this matters for comparing waveforms with measurements, not for SER or TDECQ.
- NRZ uses PRBS13. 802.3 NRZ PMDs use PRBS31 or other patterns for some tests; PRBS13 keeps the simulation grid the same as for PAM-4. Longer patterns are not available; more `pattern_periods` repeat the same pattern (they help the counted SER and the frequency resolution, not the pattern statistics).
- One alphabet per graph: all signals share one time grid (signal-spec 1).

## Parameters

| Name | Symbol | Unit | Default | Range |
|---|---|---|---|---|
| `levels` (lane: `tx.levels`) | $M$ | — | 4 | 2 (NRZ) or 4 (PAM-4) |
| `pattern` | — | — | `prbs13q` | `prbs13q` or `ssprq` (PAM-4 only) |
| `seed` (lane: `tx.prbs_seed`) | — | — | 2731 (`0x0AAB`) | nonzero 13-bit value (PRBS13Q / PRBS13 only) |
| `pattern_periods` (graph settings) | $p$ | — | 1 | ≥ 1 |
| `pattern_length` (graph settings) | — | symbols | 8191 | 8191, or 65 535 for SSPRQ |

The symbol rate (`baud`, default 53.125 GBd) and the samples per symbol (`sps`, default 16; 8 in the browser) are simulation settings of the graph, shared by all nodes.

## Validation

- Unit tests (`prbs.rs`): PRBS13 has maximal length 8191 and is balanced (4096 ones); PRBS13Q has its four symbol counts within one of each other (2047 or 2048); the NRZ pattern is exactly one PRBS13 period. PRBS31 follows its recurrence with inverted output; SSPRQ has 65 535 symbols, part 1's second half is its first half inverted, and the level counts of part 1 are symmetric (levels 0/3 and 1/2).
- Graph tests: an SSPRQ link runs on a 65 535-symbol grid and its source outputs exactly the pattern; SSPRQ on the default grid stops with a message naming `pattern_length`. Python (`tests/python/test_measured.py`): the same in a Python graph, SER below the KP4 limit.
- Not done: comparison of SSPRQ with the standard's table or a measured pattern file.
- **V15**: the NumPy reference implementation generates the same pattern; the transmitter waveforms built from it agree with Rust to < 10⁻⁹.
