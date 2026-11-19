---
component: eye-diagram
title: Eye diagram
validation: []
references: [ieee-802.3]
---

## What it models

The eye diagram (node kind `eye_diagram`): a density picture of the electrical signal at its input, like a sampling oscilloscope in persistence mode. All symbols are overlaid in a 2-UI window centred at the optimal sampling instant. It observes only (monitor inputs) and draws the noisy waveform, i.e. what a scope would capture. In the editor the node shows the eye as a thumbnail.

Inputs: `in` (electrical) and `ref`, the transmitted symbols from the pattern source (`prbs-pattern.md`), used to align the waveform and find the sampling phase in the same way as the PAM-4/NRZ analyzer (`pam4-analysis.md`).

## Equations

**Eye density** (`eq:eye-density`): the window spans $t \in [-T, T]$ around the sampling instant of every symbol ($T$ the symbol period), divided into `width` columns, and the vertical range $[v_\text{lo}, v_\text{hi}]$ into `height` rows. For each symbol $n$ the noisy waveform $y(t)$ is traced through the window with linear interpolation between samples; every histogram bin the trace crosses is counted once, so vertical gaps between columns are filled. The result is the count per bin, `counts` (rows × columns, row 0 at the top), and its maximum `max_count`.

Here $y(t)$ is the input waveform in its unit (V after the TIA), $n$ the symbol index, and $v_\text{lo}$, $v_\text{hi}$ the vertical limits (`v_range`; by default the waveform's minimum and maximum with a 5 % margin).

## Assumptions & limits

- One noise realization: the eye shows the noise of this run, not a probability contour (for those, use the analyzer's eye heights).
- Linear interpolation between samples; with few samples per symbol, fast edges look slightly straighter than they are.

## Parameters

| Name | Unit | Default | Meaning |
|---|---|---|---|
| `width` | — | 600 | columns over 2 UI |
| `height` | — | 400 | rows |
| `v_range` | signal unit | none (auto) | vertical range [low, high] |
| `phases_per_ui` | — | 64 | phase search for the sampling instant |

## Validation

- The `eye_diagram` node gives the same counts as the eye-density function applied to the lane results (identical, `tests/analysis_nodes.rs`).
