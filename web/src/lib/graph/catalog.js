// Component catalog of the graph editor: palette groups, labels, model docs, parameter
// sliders (paths into node params, SI values; display = SI / scale) and result summaries.

export const GROUPS = [
  { name: 'Sources', kinds: ['prbs13q'] },
  { name: 'Transmitter', kinds: ['driver', 'vcsel'] },
  { name: 'Fiber', kinds: ['mmf', 'custom_fiber'] },
  { name: 'Optical', kinds: ['connector', 'attenuator', 'splitter', 'combiner', 'probe'] },
  { name: 'WDM', kinds: ['wdm_mux', 'wdm_demux', 'bandpass_filter'] },
  { name: 'Receiver', kinds: ['pin', 'tia', 'bessel_thomson', 'ffe', 'receiver_output'] },
  { name: 'Analysis', kinds: ['pam4_analyzer', 'eye_diagram', 'tdecq', 'power_meter', 'optical_spectrum_analyzer', 'electrical_spectrum_analyzer'] },
  { name: 'Custom', kinds: ['rust_example'] },
];

export const LABEL = {
  prbs13q: 'PRBS pattern',
  driver: 'Driver',
  vcsel: 'VCSEL',
  connector: 'Connector',
  mmf: 'Multimode fiber',
  custom_fiber: 'Custom fiber',
  attenuator: 'Attenuator',
  splitter: 'Splitter',
  combiner: 'Combiner',
  probe: 'Probe',
  pin: 'PIN photodiode',
  tia: 'TIA',
  bessel_thomson: 'Bessel–Thomson filter',
  ffe: 'FFE equalizer',
  receiver_output: 'Receiver output',
  pam4_analyzer: 'PAM-4 / NRZ analyzer',
  eye_diagram: 'Eye diagram',
  tdecq: 'TDECQ',
  power_meter: 'Power meter',
  bandpass_filter: 'Optical bandpass filter',
  wdm_mux: 'WDM multiplexer',
  wdm_demux: 'WDM demultiplexer',
  optical_spectrum_analyzer: 'Optical spectrum analyzer',
  electrical_spectrum_analyzer: 'Electrical spectrum analyzer',
  rust_example: 'Example (Rust): gain & delay',
};

/** Shorter names in the palette, where the group heading already says what they are. */
export const PALETTE_LABEL = {
  mmf: 'Multimode', custom_fiber: 'Custom', wdm_mux: 'Multiplexer', wdm_demux: 'Demultiplexer', bandpass_filter: 'Bandpass filter',
  optical_spectrum_analyzer: 'Optical spectrum', electrical_spectrum_analyzer: 'Electrical spectrum',
  rust_example: 'Rust example (gain & delay)',
};

/** Short id prefix for new nodes. */
export const PREFIX = {
  prbs13q: 'src', driver: 'drv', vcsel: 'vcsel', connector: 'con', mmf: 'fib', custom_fiber: 'fib', attenuator: 'att',
  splitter: 'spl', combiner: 'cmb', probe: 'prb', pin: 'pin', tia: 'tia', bessel_thomson: 'rxf',
  ffe: 'ffe', receiver_output: 'rx', pam4_analyzer: 'ana', eye_diagram: 'eye', tdecq: 'tq', power_meter: 'pm',
  bandpass_filter: 'bpf', wdm_mux: 'mux', wdm_demux: 'dmx', optical_spectrum_analyzer: 'osa', electrical_spectrum_analyzer: 'esa',
  rust_example: 'ex',
};

/** Model document (docs/model/<id>.md) of each kind: one doc per component (docs/architecture.md, 5.1). */
export const DOC = {
  prbs13q: 'prbs-pattern', driver: 'driver', vcsel: 'vcsel', connector: 'connector', mmf: 'mmf', custom_fiber: 'custom-fiber',
  attenuator: 'attenuator', splitter: 'splitter', combiner: 'combiner', probe: 'probe', power_meter: 'power-meter',
  pin: 'pin', tia: 'tia', bessel_thomson: 'bessel-thomson', ffe: 'ffe', receiver_output: 'receiver-output',
  pam4_analyzer: 'pam4-analysis', eye_diagram: 'eye-diagram', tdecq: 'tdecq',
  bandpass_filter: 'bandpass-filter', wdm_mux: 'wdm-mux', wdm_demux: 'wdm-demux',
  optical_spectrum_analyzer: 'optical-spectrum-analyzer', electrical_spectrum_analyzer: 'electrical-spectrum-analyzer',
  rust_example: 'rust-example',
};

/** Signal type of a port type string ('data', 'electrical', 'electrical (current)', 'optical'). */
export const domain = (ty) => (ty.startsWith('electrical') ? 'electrical' : ty);

/** Can an output of type `from` drive an input of type `to`? (mirrors PortType::connects_to) */
export function connects(from, to) {
  if (domain(from) !== domain(to)) return false;
  if (domain(from) !== 'electrical') return true;
  const q = (t) => (t === 'electrical' ? null : t.slice('electrical ('.length, -1));
  const [a, b] = [q(from), q(to)];
  return b === null || (a !== null && a === b);
}

const s = (path, label, unit, min, max, step, digits, scale) => ({ path, label, unit, min, max, step, digits, scale });

export const SPECS = {
  driver: [
    s('rj_rms', 'Random jitter (RMS)', 'ps', 0, 2, 0.01, 2, 1e-12),
    s('dcd', 'Duty-cycle distortion (p-p)', 'ps', 0, 20, 0.1, 1, 1e-12),
    s('sj_amplitude', 'Sinusoidal jitter (peak)', 'ps', 0, 20, 0.1, 1, 1e-12),
    s('sj_frequency', 'SJ frequency', 'MHz', 1, 2000, 1, 0, 1e6),
    s('cdr_corner', 'Clock recovery corner', 'MHz', 0, 20, 0.1, 1, 1e6),
  ],
  rust_example: [
    s('gain', 'Gain g', '', 0, 2, 0.01, 2),
    s('delay', 'Delay τ', 'ps', 0, 20, 0.1, 1, 1e-12),
  ],
  pam4_analyzer: [s('clock_rj_rms_ui', 'Receiver clock jitter (RMS)', 'mUI', 0, 50, 0.5, 1, 1e-3)],
  vcsel: [
    s('p_avg_dbm', 'Average power', 'dBm', -8, 4, 0.1, 1),
    s('er_db', 'Extinction ratio', 'dB', 2, 8, 0.1, 1),
    s('vcsel.resonance_freq', 'Resonance f_r', 'GHz', 12, 40, 0.5, 1, 1e9),
    s('vcsel.damping', 'Damping ζ', '', 0.3, 1.5, 0.01, 2),
    s('vcsel.rin_db_hz', 'RIN', 'dB/Hz', -155, -125, 1, 0),
    s('vcsel.spectral_width_rms', 'Spectral width (RMS)', 'nm', 0, 0.65, 0.01, 2, 1e-9),
    s('vcsel.wavelength', 'Wavelength', 'nm', 840, 950, 1, 0, 1e-9),
    s('beam_radius', 'VCSEL mode waist w', 'µm', 2, 10, 0.1, 1, 1e-6),
  ],
  connector: [
    s('offset', 'Radial offset', 'µm', 0, 25, 0.25, 2, 1e-6),
    s('tilt', 'Tilt', 'mrad', 0, 150, 1, 0, 1e-3),
    s('loss_db', 'Insertion loss', 'dB', 0, 3, 0.05, 2),
  ],
  // A multimode preset (mmf) shows the same sliders; changing anything but length, attenuation
  // (≤ 3.5 dB/km) and extra loss converts it to a custom fiber (FIBER_PHYSICS).
  custom_fiber: [
    s('length', 'Span length', 'm', 0, 400, 1, 0),
    s('attenuation_db_per_km', 'Attenuation', 'dB/km', 0, 5, 0.1, 1),
    s('extra_loss_db', 'Extra loss', 'dB', 0, 3, 0.1, 1),
    s('profile.alpha', 'Profile α', '', 1.9, 2.2, 0.0005, 4),
    s('core_radius', 'Core radius', 'µm', 2, 50, 0.25, 2, 1e-6),
    s('na', 'NA (at 850 nm)', '', 0.08, 0.3, 0.005, 3),
    s('grid_points_per_radius', 'Solver grid / radius', '', 300, 1200, 100, 0),
    s('domain_factor', 'Solver domain', '× a', 1.5, 3, 0.1, 1),
  ],
  attenuator: [s('loss_db', 'Attenuation', 'dB', 0, 20, 0.1, 1)],
  splitter: [s('excess_loss_db', 'Excess loss', 'dB', 0, 3, 0.05, 2)],
  combiner: [s('excess_loss_db', 'Excess loss', 'dB', 0, 3, 0.05, 2)],
  probe: [
    s('tap_ratio', 'Tap ratio (realistic)', '', 0, 0.5, 0.01, 2),
    s('insertion_loss_db', 'Insertion loss (realistic)', 'dB', 0, 2, 0.05, 2),
  ],
  pin: [s('responsivity', 'Responsivity', 'A/W', 0.3, 0.65, 0.01, 2)],
  tia: [
    s('transimpedance', 'Transimpedance Z', 'Ω', 200, 3000, 10, 0),
    s('noise_density', 'Noise density', 'pA/√Hz', 5, 200, 0.5, 1, 1e-12),
  ],
  bessel_thomson: [s('f3db_rel', 'Bandwidth', '× baud', 0.3, 1.0, 0.01, 2)],
  ffe: [s('n_taps', 'Taps', '', 1, 15, 1, 0)],
  eye_diagram: [],
  bandpass_filter: [
    s('center', 'Centre wavelength', 'nm', 820, 980, 0.5, 1, 1e-9),
    s('width', 'Width (−3 dB)', 'nm', 1, 60, 0.5, 1, 1e-9),
    s('order', 'Edge steepness (order)', '', 1, 8, 1, 0),
    s('insertion_loss_db', 'Insertion loss', 'dB', 0, 5, 0.05, 2),
    s('isolation_db', 'Isolation', 'dB', 5, 60, 0.5, 1),
  ],
  wdm_mux: [
    s('width', 'Passband width (−3 dB)', 'nm', 4, 30, 0.5, 1, 1e-9),
    s('order', 'Edge steepness (order)', '', 1, 8, 1, 0),
    s('insertion_loss_db', 'Insertion loss', 'dB', 0, 5, 0.05, 2),
    s('isolation_db', 'Isolation (crosstalk floor)', 'dB', 5, 60, 0.5, 1),
  ],
  optical_spectrum_analyzer: [
    s('start', 'Start', 'nm', 780, 1000, 1, 0, 1e-9),
    s('stop', 'Stop', 'nm', 800, 1100, 1, 0, 1e-9),
    s('rbw', 'Resolution bandwidth', 'nm', 0.02, 2, 0.01, 2, 1e-9),
  ],
  electrical_spectrum_analyzer: [
    s('stop_rel', 'Stop frequency', '× baud', 0.2, 4, 0.05, 2),
    s('points', 'Points', '', 50, 2000, 10, 0),
  ],
};
SPECS.wdm_demux = SPECS.wdm_mux;

SPECS.mmf = SPECS.custom_fiber;

/** Multimode fiber types of the `mmf` preset (values come from the engine, fiber_presets). */
export const MMF_TYPES = ['om3', 'om4', 'om5'];
/** Parameters an `mmf` preset owns; everything else is fiber physics. */
export const MMF_KEYS = ['type', 'length', 'attenuation_db_per_km', 'extra_loss_db'];
/** Cabled attenuation limit of OM3/OM4/OM5 at 850 nm [dB/km]. */
export const MMF_MAX_ATTENUATION = 3.5;

/** True if setting `path` = `value` on an mmf preset changes its physics (→ custom fiber). */
export function isFiberPhysics(path, value) {
  if (path === 'attenuation_db_per_km') return value > MMF_MAX_ATTENUATION;
  return !MMF_KEYS.includes(path.split('.')[0]);
}

/** Boolean switches per kind. */
export const SWITCHES = {
  vcsel: [['rin_enabled', 'RIN noise']],
  probe: [['realistic', 'Realistic tap (ratio and insertion loss)']],
  pin: [['shot_noise', 'Shot noise']],
  tia: [['thermal_noise', 'Thermal noise']],
  bessel_thomson: [['enabled', 'Filter enabled']],
  ffe: [['noise_aware', 'Noise-aware adaptation']],
};

const fmt = (v, d = 2) => (v === null || v === undefined || !isFinite(v) ? '—' : Number(v).toFixed(d));
const exp = (v) => (v === null || v === undefined ? '—' : v === 0 ? '0' : Number(v).toExponential(1));
const dbm = (w) => (w > 0 ? `${(10 * Math.log10(w / 1e-3)).toFixed(2)} dBm` : '—');
const mhzkm = (v) => (v ? `${(v / 1e9).toFixed(0)} MHz·km` : '—');
const nm = (v, d = 1) => (v === null || v === undefined ? '—' : `${(v * 1e9).toFixed(d)} nm`);

/** Channel plan of a WDM node as text, e.g. "4 ch · 850…940 nm". */
export function channelText(channels) {
  const c = channels ?? [850e-9, 880e-9, 910e-9, 940e-9];
  if (!c.length) return 'no channels';
  return `${c.length} ch · ${(c[0] * 1e9).toFixed(0)}${c.length > 1 ? `…${(c.at(-1) * 1e9).toFixed(0)}` : ''} nm`;
}

/** Value with an SI prefix, e.g. si(3.2e-4, 'A') = "320 µA". */
export function si(v, unit = '', digits = 3) {
  if (v === null || v === undefined || !isFinite(v)) return '—';
  if (v === 0) return `0${unit ? ' ' + unit : ''}`;
  if (!unit) return Math.abs(v) < 1e-3 || Math.abs(v) >= 1e4 ? v.toExponential(digits - 1) : String(+v.toPrecision(digits));
  const p = [[1e9, 'G'], [1e6, 'M'], [1e3, 'k'], [1, ''], [1e-3, 'm'], [1e-6, 'µ'], [1e-9, 'n'], [1e-12, 'p'], [1e-15, 'f']];
  const [s, pre] = p.find(([s]) => Math.abs(v) >= s * 0.9995) ?? p.at(-1);
  return `${+(v / s).toPrecision(digits)} ${pre}${unit}`.trim();
}

const QUANTITY_UNIT = { current: 'A', voltage: 'V', dimensionless: '' };

/**
 * Port summary of a run (results `ports` section) as short text:
 * optical "−1.23 dBm"; electrical "mean 1.2 mV · swing 40 mV · noise 1.1 mV rms";
 * data "8191 symbols".
 */
export function portText(s) {
  if (!s) return '';
  switch (s.type) {
    case 'optical':
      return dbm(s.average_power);
    case 'electrical': {
      const u = QUANTITY_UNIT[s.quantity] ?? '';
      return `mean ${si(s.mean, u)} · swing ${si(s.swing, u)} · noise ${si(s.noise_rms, u)} rms${s.gaussian_exact ? '' : ' (non-Gaussian)'}`;
    }
    case 'data':
      return `${s.symbols} symbols`;
    default:
      return '';
  }
}

/** Configured transmitter jitter of a driver, e.g. "RJ 0.3 ps · DCD 2 ps · SJ 5 ps @ 100 MHz". */
export function jitterText(p) {
  if (!p) return '';
  const ps = (v) => `${+(v * 1e12).toPrecision(3)} ps`;
  const parts = [];
  if (p.rj_rms > 0) parts.push(`RJ ${ps(p.rj_rms)}`);
  if (p.dcd > 0) parts.push(`DCD ${ps(p.dcd)}`);
  if (p.sj_amplitude > 0) parts.push(`SJ ${ps(p.sj_amplitude)} @ ${si(p.sj_frequency ?? 100e6, 'Hz')}`);
  return parts.length ? parts.join(' · ') : 'no jitter';
}

/**
 * SER and BER of an analyzer report as text. A 0 is shown as what it means: below the
 * method's floor (semi-analytic: 2·Q(9) ≈ 2e-19; counted: 1/N), or exactly 0 without noise.
 */
export function errorRates(r) {
  if (!r || r.ser === null || r.ser === undefined) return { ser: '—', ber: '—' };
  const counted = r.ser_method === 'monte_carlo';
  if (r.ser > 0) return { ser: exp(r.ser) + (counted ? ' (counted)' : ''), ber: exp(r.ber) };
  if (!counted && r.noise_rms === 0) return { ser: '0 (no noise)', ber: '0 (no noise)' };
  const floor = r.ser_floor ?? (counted ? r.ser_resolution : null);
  if (!floor) return { ser: '0', ber: '0' };
  // NRZ: BER = SER; PAM-4: BER ≈ SER/2.
  const ber = r.level_mean?.length === 2 ? floor : floor / 2;
  return { ser: `< ${exp(floor)}${counted ? ' (counted)' : ''}`, ber: `< ${exp(ber)}` };
}

/**
 * Pre-FEC BER limit of 100G-per-lane PAM-4 Ethernet: links run RS(544,514) "KP4" FEC, and
 * the PMDs are specified for BER ≤ 2.4 × 10⁻⁴ before it (IEEE 802.3, e.g. Clauses 138, 167).
 */
export const KP4_BER_LIMIT = 2.4e-4;

/** Margin of a PAM-4 analyzer result to the KP4 limit: `{ ok, text }`, or null for NRZ. */
export function fecMargin(r) {
  if (!r || r.level_mean?.length !== 4 || r.ber === null || r.ber === undefined) return null;
  const counted = r.ser_method === 'monte_carlo';
  const floor = r.ser_floor ?? (counted ? r.ser_resolution : null);
  const ber = r.ber > 0 ? r.ber : floor ? floor / 2 : 0;
  if (ber === 0) return { ok: true, text: 'OK (no errors)' };
  const m = KP4_BER_LIMIT / ber;
  const f = (x) => (x >= 100 ? x.toExponential(0).replace('e+', 'e') : x.toFixed(x >= 10 ? 0 : 1));
  if (m >= 1) return { ok: true, text: `OK ×${f(m)}${r.ber > 0 ? '' : '+'}` };
  return { ok: false, text: `FAIL ×${f(1 / m)} over` };
}

/**
 * Instrument display for measuring nodes (shown like the readout of a real instrument):
 * { main: { label, value, unit }, rows: [[label, value]] }, or null for other kinds or
 * before the first run.
 */
export function display(kind, params, r) {
  if (!r) return null;
  switch (kind) {
    case 'power_meter': {
      const w = r.average_power;
      return {
        main: { label: 'AVG POWER', value: w > 0 ? (10 * Math.log10(w / 1e-3)).toFixed(2) : '—', unit: 'dBm' },
        rows: [['P', si(w, 'W')]],
      };
    }
    case 'pam4_analyzer': {
      const e = errorRates(r);
      const nrz = r.level_mean?.length === 2;
      const fec = fecMargin(r);
      return {
        main: { label: `${nrz ? 'NRZ' : 'PAM-4'} SER${nrz ? '' : ' · pre-FEC'}${r.ser_method === 'monte_carlo' ? ' (counted)' : ''}`, value: e.ser.replace(' (counted)', ''), unit: '' },
        rows: [
          ['BER', e.ber],
          ...(fec ? [['KP4 2.4e-4', fec.text]] : []),
          ['Q', fmt(r.q_min, 2)],
          ['EH', `${r.eye_height.map((x) => (x * 1e3).toFixed(0)).join('/')} mV`],
          ['EW', `${r.eye_width.map((x) => x.toFixed(2)).join('/')} UI`],
        ],
      };
    }
    case 'optical_spectrum_analyzer':
      return {
        main: { label: 'PEAK', value: fmt(r.peak?.power_dbm), unit: 'dBm' },
        rows: [
          ['λ', nm(r.peak?.wavelength, 2)],
          ['Total', `${fmt(r.total_power_dbm)} dBm`],
          ['Sources', String(r.sources?.length ?? 0)],
          ['RBW', nm(r.rbw, 2)],
        ],
      };
    case 'electrical_spectrum_analyzer':
      return {
        main: { label: 'SNR', value: fmt(r.snr_db, 1), unit: 'dB' },
        rows: [
          ['Signal', `${si(r.signal_rms, r.unit === '1' ? '' : r.unit)} rms`],
          ['Noise', `${si(r.noise_rms, r.unit === '1' ? '' : r.unit)} rms`],
          ['RBW', si(r.rbw, 'Hz')],
        ],
      };
    case 'tdecq':
      return {
        main: { label: 'TDECQ', value: r.tdecq_db === null ? 'CLOSED' : fmt(r.tdecq_db), unit: r.tdecq_db === null ? '' : 'dB' },
        rows: [
          ['C_eq', `${fmt(r.ceq_db)} dB`],
          ['OMA', r.oma_outer > 0 ? `${(10 * Math.log10(r.oma_outer / 1e-3)).toFixed(2)} dBm` : '—'],
        ],
      };
    default:
      return null;
  }
}

/**
 * Trace for the node thumbnail of a spectrum analyzer: { x, traces: [{ y, color }], range,
 * xlabel }, or null.
 */
export function trace(kind, r) {
  if (!r) return null;
  if (kind === 'optical_spectrum_analyzer' && r.power_dbm?.length) {
    const top = Math.ceil((r.peak.power_dbm + 2) / 10) * 10;
    return {
      x: r.wavelength,
      traces: [{ y: r.power_dbm, color: '#f08a5d' }],
      range: [top - 60, top],
      xlabel: `${(r.wavelength[0] * 1e9).toFixed(0)}–${(r.wavelength.at(-1) * 1e9).toFixed(0)} nm · 10 dB/div`,
    };
  }
  if (kind === 'electrical_spectrum_analyzer' && r.signal_db?.length) {
    const all = [...r.signal_db, ...r.noise_db].filter(isFinite);
    const top = Math.ceil(Math.max(...all) / 10) * 10;
    return {
      x: r.frequency,
      traces: [{ y: r.signal_db, color: '#6cb6ff' }, { y: r.noise_db, color: '#8b94a7' }],
      range: [top - 80, top],
      xlabel: `0–${(r.frequency.at(-1) / 1e9).toFixed(0)} GHz · signal / noise · 20 dB/div`,
    };
  }
  return null;
}

/** One-line result shown on the node. */
export function badge(kind, params, r) {
  switch (kind) {
    case 'prbs13q':
      return (params.levels ?? 4) === 2 ? 'NRZ (PRBS13)' : 'PAM-4 (PRBS13Q)';
    case 'mmf':
      return `${fmt(params.length ?? 100, 0)} m · ${(params.type ?? 'om4').toUpperCase()}${r?.emb_length ? ` · EMB ${mhzkm(r.emb_length)}` : ''}`;
    case 'custom_fiber':
      return `${fmt(params.length, 0)} m · custom${r?.emb_length ? ` · EMB ${mhzkm(r.emb_length)}` : ''}`;
    case 'connector':
      return `Δx ${fmt((params.offset ?? 0) * 1e6, 1)} µm · ${fmt(params.loss_db, 2)} dB`;
    case 'attenuator':
      return `${fmt(params.loss_db, 1)} dB`;
    case 'rust_example':
      return `g ${fmt(params.gain ?? 1, 2)} · τ ${fmt((params.delay ?? 0) * 1e12, 1)} ps`;
    case 'vcsel':
      return r ? `${dbm(r.average_power)} · ${fmt(r.vcsel_bandwidth / 1e9, 1)} GHz` : `${fmt(params.p_avg_dbm, 1)} dBm`;
    case 'pin':
      return r ? `Rx ${dbm(r.average_power)}` : '';
    case 'bessel_thomson':
      return `${fmt(params.f3db_rel, 2)} × baud`;
    case 'tia':
      return params.clip ? 'clipping (nonlinear)' : `${fmt(params.transimpedance, 0)} Ω`;
    case 'power_meter':
      return r ? dbm(r.average_power) : '';
    case 'pam4_analyzer':
      return r
        ? [
            `SER ${errorRates(r).ser}${r.level_mean?.length === 4 ? ' (pre-FEC)' : ''}`,
            `BER ${errorRates(r).ber} · Q ${fmt(r.q_min, 1)}${fecMargin(r) ? ` · KP4 ${fecMargin(r).text}` : ''}`,
            `EH ${r.eye_height.map((x) => (x * 1e3).toFixed(0)).join(' / ')} mV`,
            `EW ${r.eye_width.map((x) => x.toFixed(2)).join(' / ')} UI`,
          ]
        : '';
    case 'tdecq':
      return r ? (r.tdecq_db === null ? 'eye closed' : `${fmt(r.tdecq_db)} dB`) : '';
    case 'ffe':
      return r ? `${r.taps.length} taps${r.adapted ? ', adapted' : ''}` : '';
    case 'splitter':
      return (params.ratios ?? []).map((x) => fmt(x, 2)).join(' : ');
    case 'driver':
      return jitterText(params);
    case 'bandpass_filter':
      return `${nm(params.center ?? 850e-9, 0)} · ${nm(params.width ?? 22e-9, 0)} wide · ${fmt(params.insertion_loss_db ?? 0.5, 1)} dB`;
    case 'wdm_mux':
    case 'wdm_demux':
      return `${channelText(params.channels)} · ${fmt(params.insertion_loss_db ?? 1, 1)} dB`;
    case 'receiver_output':
      return r
        ? [`μ ${si(r.mean, 'V')} · σ ${si(r.sigma_rms, 'V')} rms`, r.gaussian_exact ? 'Gaussian, σ(t) exact' : 'non-Gaussian (counted SER)']
        : '';
    case 'eye_diagram':
      return r ? `${si(r.v_min, 'V', 2)} … ${si(r.v_max, 'V', 2)} · ${fmt(r.phase, 2)} UI` : '';
    default:
      return '';
  }
}

/** Live values for the math panel: [label, value] rows. */
export function liveValues(kind, r) {
  if (!r) return [];
  switch (r.type) {
    case 'transmitter':
      return [
        [`Levels P₀…P${'₀₁₂₃'[r.levels.length - 1] ?? r.levels.length - 1}`, r.levels.map((x) => (x * 1e3).toFixed(3)).join(', ') + ' mW'],
        ['Average power', dbm(r.average_power)],
        ['VCSEL −3 dB bandwidth', `${fmt(r.vcsel_bandwidth / 1e9, 1)} GHz`],
      ];
    case 'fiber_span':
      return [
        ['Guided LP modes', String(r.modes)],
        ['V-number', fmt(r.v_number, 2)],
        ...(r.emb_length ? [['Modal bandwidth, this launch', mhzkm(r.emb_length)], ['OFL bandwidth', mhzkm(r.ofl_bandwidth_length)], ['Guided fraction', fmt(r.guided_fraction, 4)]] : []),
      ];
    case 'detection':
      return [
        ['Detected power', dbm(r.average_power)],
        ...r.channels.map((c, i) => [`Source ${i + 1}: transmission / RMS delay spread`, `${fmt(-10 * Math.log10(c.transmission))} dB / ${fmt(c.rms_delay_spread * 1e12)} ps`]),
      ];
    case 'filter':
      return [['Noise bandwidth B_n', `${fmt(r.noise_bandwidth / 1e9, 1)} GHz`]];
    case 'receiver':
      return [['Noise statistics', r.gaussian_exact ? 'Gaussian, σ(t) exact' : 'after a nonlinear block (counted SER)'], ['Mean (noiseless)', si(r.mean, 'V')], ['RMS noise σ', `${fmt(r.sigma_rms * 1e3)} mV`]];
    case 'power_meter':
      return [['Average power', dbm(r.average_power)]];
    case 'pam4_analysis':
      return [
        ['SER', `${errorRates(r).ser.replace(' (counted)', '')} (${r.ser_method === 'monte_carlo' ? 'counted' : 'semi-analytic'})`],
        [r.level_mean.length === 2 ? 'BER = SER (NRZ)' : 'BER ≈ SER/2 (Gray)', errorRates(r).ber],
        ['Q-factor (worst level pair)', fmt(r.q_min, 2)],
        ['Eye heights (10⁻³)', r.eye_height.map((x) => (x * 1e3).toFixed(1)).join(' / ') + ' mV'],
        ['Eye widths', r.eye_width.map((x) => x.toFixed(2)).join(' / ') + ' UI'],
        ['Sampling phase', `${fmt(r.phase, 3)} UI`],
      ];
    case 'wdm':
      return r.ports.flatMap((pt) => pt.sources.map((x) => [`${pt.port} (${nm(pt.center, 0)}): ${x.source} at ${nm(x.wavelength, 0)}`, `${fmt(x.transmission_db, 2)} dB${x.in_band ? '' : ' (out of band)'}`]));
    case 'optical_spectrum':
      return [
        ['Total power', `${fmt(r.total_power_dbm)} dBm`],
        ['Peak', `${fmt(r.peak.power_dbm)} dBm at ${nm(r.peak.wavelength, 2)} (per ${nm(r.rbw, 2)} RBW)`],
        ...r.sources.map((x) => [`${x.source}: ${nm(x.wavelength, 1)}, σ ${nm(x.spectral_width_rms, 2)}`, `${fmt(x.power_dbm)} dBm`]),
        ['Model', 'Gaussian line per source; no mode structure or modulation sidebands'],
      ];
    case 'electrical_spectrum':
      return [
        ['Signal (AC) / noise', `${si(r.signal_rms, r.unit === '1' ? '' : r.unit)} / ${si(r.noise_rms, r.unit === '1' ? '' : r.unit)} rms`],
        ['SNR', `${fmt(r.snr_db, 1)} dB`],
        ['DC', si(r.dc, r.unit === '1' ? '' : r.unit)],
        ['Resolution bandwidth', si(r.rbw, 'Hz')],
      ];
    case 'tdecq':
      return [
        ['TDECQ', r.tdecq_db === null ? 'eye closed' : `${fmt(r.tdecq_db)} dB`],
        ['C_eq', `${fmt(r.ceq_db)} dB`],
        ['Reference FFE taps', r.taps.map((x) => x.toFixed(3)).join(', ')],
      ];
    case 'ffe':
      return [
        ['Taps', r.taps.map((x) => x.toFixed(3)).join(', ')],
        ['Main cursor', String(r.main)],
        ...(r.adapted ? [['Design phase', `${fmt(r.phase, 3)} UI`], ['MSE', exp(r.mse)]] : [['Taps', 'fixed']]),
      ];
    case 'eye_diagram':
      return [['Sampling phase', `${fmt(r.phase, 3)} UI`], ['Range', `${fmt(r.v_min * 1e3, 0)} … ${fmt(r.v_max * 1e3, 0)} mV`]];
    default:
      return [];
  }
}

/** Zones of the system view, used to suggest frames: zone of each kind. */
export const ZONES = ['Transmitter', 'Channel', 'Receiver front end', 'Data recovery'];
export const ZONE_OF = {
  prbs13q: 'Transmitter', driver: 'Transmitter', vcsel: 'Transmitter',
  connector: 'Channel', mmf: 'Channel', custom_fiber: 'Channel', attenuator: 'Channel', splitter: 'Channel', combiner: 'Channel', probe: 'Channel', power_meter: 'Channel',
  pin: 'Receiver front end', tia: 'Receiver front end', bessel_thomson: 'Receiver front end',
  ffe: 'Data recovery', receiver_output: 'Data recovery', pam4_analyzer: 'Data recovery', eye_diagram: 'Data recovery', tdecq: 'Data recovery',
  bandpass_filter: 'Channel', wdm_mux: 'Channel', wdm_demux: 'Channel', optical_spectrum_analyzer: 'Channel', electrical_spectrum_analyzer: 'Data recovery',
};
