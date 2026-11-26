# Measured data: analyzing a scope capture with Lightstone

Lightstone can read a waveform captured on a sampling oscilloscope and analyze it like a simulated one: TDECQ (with the standard 5-tap reference equalizer, another tap count, or none), PAM-4/NRZ eye heights and widths, and the eye diagram. This is the first step of comparing Lightstone with a real link: if Lightstone's TDECQ on the very same waveform agrees with the scope's, the analysis is right, and any later difference between a simulation and a measurement comes from the device model, not from the math.

## What you need

| Item | Notes |
|---|---|
| The captured waveform | Ideally an averaged, pattern-locked capture of one or more whole pattern periods (the usual TDECQ capture). CSV (one column of values, or two columns time [s] and value), `.npy`, or raw binary. Any unit (W, V, ADC counts): TDECQ and the eye metrics are ratios. |
| The sample rate | From the time column, or given as `sample_rate` [Hz]. |
| The symbol rate | `baud` [Bd], e.g. 53.125e9. |
| The transmitted symbols | The pattern of one period: `"ssprq"` or `"prbs13q"`, an array of 0…3, or a text file with one symbol per line (or separated by commas/spaces). **For SSPRQ, prefer the pattern file of your scope or pattern generator**: Lightstone's own SSPRQ is not yet verified against the standard (`model/prbs-pattern.md`). |
| The scope's TDECQ settings | Whether the scope already applied the reference receiver (TDECQ mode, 4th-order Bessel–Thomson at 0.5 × baud), and its equalizer taps. |

## Usage (Python)

```python
import lightstone

m = lightstone.read_waveform("capture.csv", baud=53.125e9, symbols="ssprq_pattern.txt")
print(m.info["method"], m.info["periods_captured"])

t = m.tdecq()                       # standard: 5-tap reference equalizer
print(t["tdecq_db"], t["ceq_db"], t["taps"])
print(m.tdecq(ffe_taps=1)["tdecq_db"])   # without equalizer (None if the eye is closed)

a = m.analyze()                     # eye heights and widths, counted SER
counts, meta = m.eye()              # eye density
```

From arrays instead of a file: `lightstone.measured_waveform(samples, sample_rate, baud, symbols)`.

Options:
- `reference_filter=True` applies the TDECQ reference receiver (4th-order Bessel–Thomson at `f3db_rel` × baud, default 0.5) to the capture. Use it only if the scope did **not** already filter: a TDECQ-mode capture is already filtered.
- `sps` (default 16): samples per symbol on Lightstone's grid.
- `start` [s]: where the period to use begins, for a capture that is not a whole number of periods.
- TDECQ options as for simulated waveforms: `ffe_taps` (1 … 15, odd), `oma_outer` (if you measured OMA_outer separately, e.g. on a square wave), `delay`.

## How the capture is mapped onto the grid

The pattern period is $N_\text{sym}/R_s$, with $N_\text{sym}$ the number of symbols of one period and $R_s$ the symbol rate. Lightstone's grid has $N_\text{sym} \cdot s$ samples for $s$ samples per symbol.

- If the capture spans a whole number of periods (within 0.1 %), it is resampled band-limited (FFT) to the grid and the periods are averaged. This is exact for a periodic, band-limited waveform.
- Otherwise one period from `start` is taken by linear interpolation; `info["method"]` says which method was used. Linear interpolation is accurate when the capture has many samples per symbol (≥ 16).
- The symbol alignment is found by cross-correlation with the symbols, as for simulated waveforms. The capture may start anywhere within the pattern.
- Noise: an averaged capture contains little noise; TDECQ adds its own (σ_G), so averaging is what the standard intends. The SER from `analyze()` is counted on the waveform itself and is not meaningful for an averaged capture.

## Checks (`tests/python/test_measured.py`)

- A simulated reference-receiver waveform (50 m OM4), resampled to 23 and 40 samples per UI and to one and three periods, gives back its own TDECQ within 0.01 dB (measured: identical to 10⁻⁴ dB). A capture of 1.3 periods (linear interpolation) is within 0.05 dB.
- The same through CSV (with header, time column, symbols file) and `.npy`.
- The import's reference filter equals the NumPy reference's Bessel–Thomson response to 10⁻¹².
- Not done yet: a real scope capture with the scope's own TDECQ. If you have one, please [open an issue](https://github.com/lightstone-photonics/lightstone/issues).

## Limits

- One lane, one capture. Real-time scopes capturing millions of symbols with random data work only if the symbol sequence is known and the capture is cut to whole periods.
- Clock: the symbol rate must match the capture exactly (a pattern-locked capture does). Drift over the capture is not corrected.
- The capture is taken as it is: no de-embedding of the O/E converter or scope response beyond the optional reference filter.
