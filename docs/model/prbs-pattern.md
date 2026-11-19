---
component: prbs-pattern
title: Pattern source (PRBS13Q / PRBS13)
validation: [V15]
references: [ieee-802.3]
---

## What it models

The test data of the link: a pseudo-random symbol sequence, as used to specify and measure optical PAM-4 and NRZ links. Every short combination of symbols occurs (almost) equally often, so the pattern exercises all transitions the link has to carry. Node kind `prbs13q`; its output is a data signal (symbols and alphabet size), with no noise. The driver turns it into a drive waveform (`driver.md`); the analyzers, the FFE and TDECQ use it as the reference sequence (`ref` inputs).

## Equations

**PRBS13** (`eq:tx-prbs13`): a 13-bit linear-feedback shift register with polynomial $1 + x + x^2 + x^{12} + x^{13}$. Here $x^k$ stands for a delay of $k$ bits: each new bit is the XOR (sum modulo 2) of the bits 1, 2, 12 and 13 positions earlier. The register starts from a nonzero 13-bit seed and runs through all $2^{13} - 1 = 8191$ nonzero states, so the bit sequence repeats after 8191 bits (maximal length).

**PRBS13Q** (`eq:tx-gray`), for PAM-4 (`levels: 4`, default): two PRBS13 periods (16 382 bits) are taken in pairs, the first bit as MSB, and Gray-coded to a symbol $k \in \{0, 1, 2, 3\}$:

$$00 \to 0,\qquad 01 \to 1,\qquad 11 \to 2,\qquad 10 \to 3$$

Neighbouring symbols differ in one bit, so a symbol error to the next level costs one bit error (BER ≈ SER/2).

**NRZ** (`levels: 2`): one PRBS13 period of 8191 bits is used directly as symbols $k \in \{0, 1\}$.

Both patterns therefore have $N_\text{sym} = 8191$ symbols per period, and the simulation grid is the same for both. With `pattern_periods` $= p$ the source repeats the pattern $p$ times, so $N_\text{sym} = 8191\,p$; the record is one period of an endlessly repeating signal (signal-spec 1).

## Assumptions & limits

- The seed and bit order of PRBS13Q have not been checked bit for bit against IEEE 802.3 Clause 120.5.11.2.1. Results depend on the pattern only through its statistics, so this matters for comparing waveforms with measurements, not for SER or TDECQ.
- NRZ uses PRBS13. 802.3 NRZ PMDs use PRBS31 or other patterns for some tests; PRBS13 keeps the simulation grid the same as for PAM-4. Longer patterns are not available; more `pattern_periods` repeat the same pattern (they help the counted SER and the frequency resolution, not the pattern statistics).
- One alphabet per graph: all signals share one time grid (signal-spec 1).

## Parameters

| Name | Symbol | Unit | Default | Range |
|---|---|---|---|---|
| `levels` (lane: `tx.levels`) | $M$ | — | 4 | 2 (NRZ) or 4 (PAM-4) |
| `seed` (lane: `tx.prbs_seed`) | — | — | 2731 (`0x0AAB`) | nonzero 13-bit value |
| `pattern_periods` (graph settings) | $p$ | — | 1 | ≥ 1 |

The symbol rate (`baud`, default 53.125 GBd) and the samples per symbol (`sps`, default 16; 8 in the browser) are simulation settings of the graph, shared by all nodes.

## Validation

- Unit tests (`prbs.rs`): PRBS13 has maximal length 8191 and is balanced (4096 ones); PRBS13Q has its four symbol counts within one of each other (2047 or 2048); the NRZ pattern is exactly one PRBS13 period.
- **V15**: the NumPy reference implementation generates the same pattern; the transmitter waveforms built from it agree with Rust to < 10⁻⁹.
