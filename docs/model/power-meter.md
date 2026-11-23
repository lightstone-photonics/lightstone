---
component: power-meter
title: Power meter
validation: [G4]
references: []
---

## What it models

An optical power meter (node kind `power_meter`): it reports the average optical power at its input. Its input is a monitor input, so it can be wired to an optical output that already feeds a component; it observes without taking light. In the editor it shows an instrument readout in dBm.

## Equations

**Average power** (`eq:power-meter`): the time average of the optical power with all light collected, summed over all source contributions,

$$\bar P = \frac{1}{T_\text{p}}\int_0^{T_\text{p}} \sum_\text{sources}\sum_k P_k(t)\thinspace dt$$

Here $\bar P$ is the average optical power (W) that the meter reports, $T_\text{p}$ one simulation period (s), $t$ time, the outer sum runs over all source contributions (e.g. several lasers joined by a combiner), and $k$ runs over the mode bins of a contribution, with $P_k(t)$ the power (W) in mode bin $k$.

## Assumptions & limits

- No wavelength or responsivity dependence and no detector area: it measures all guided power.
- No noise of its own and no averaging time other than the simulation period.

## Parameters

None.

## Validation

- **G4** (`probe.md`): behind a realistic probe's tap it reads exactly the tap ratio times the input power (to 10⁻¹²).
