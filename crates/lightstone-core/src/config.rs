//! Link configuration. All fields are SI unless the name says otherwise
//! (`*_dbm`, `*_db`, `*_db_per_km`, `*_db_hz` are the only engineering-unit fields).

use serde::{Deserialize, Serialize};

/// Transmitter / pattern settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TxConfig {
    /// Symbol rate \[Bd\].
    pub baud: f64,
    /// Samples per symbol (integer oversampling).
    pub sps: usize,
    /// Number of pattern periods (8191 symbols each).
    pub pattern_periods: usize,
    /// PRBS13 seed (13-bit, nonzero).
    pub prbs_seed: u16,
    /// Average launched optical power \[dBm\].
    pub p_avg_dbm: f64,
    /// Outer extinction ratio P_{M−1}/P₀ \[dB\].
    pub er_db: f64,
    /// Alphabet size M: 4 = PAM-4 (PRBS13Q), 2 = NRZ (PRBS13).
    pub levels: u8,
    /// Transmitter jitter (all zero: none).
    pub jitter: JitterConfig,
}

/// Transmitter jitter at the driver (`eq:tx-jitter`). Times in seconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JitterConfig {
    /// RMS random jitter per edge \[s\] (Gaussian, independent edges), modeled as an
    /// equivalent slope noise.
    pub rj_rms: f64,
    /// Duty-cycle distortion, peak-to-peak \[s\]: rising edges dcd/2 early, falling edges
    /// dcd/2 late (high pulses dcd longer).
    pub dcd: f64,
    /// Sinusoidal jitter amplitude (peak) \[s\].
    pub sj_amplitude: f64,
    /// Sinusoidal jitter frequency \[Hz\], rounded to a multiple of 1/record length.
    pub sj_frequency: f64,
    /// Clock-recovery corner frequency \[Hz\]: sinusoidal jitter is referenced to the
    /// recovered clock (first-order tracking), i.e. multiplied by the jitter transfer
    /// 1 − H_CDR(f) = jf/(f_c + jf). 0 = no clock recovery.
    pub cdr_corner: f64,
}

impl Default for JitterConfig {
    fn default() -> Self {
        Self {
            rj_rms: 0.0,
            dcd: 0.0,
            sj_amplitude: 0.0,
            sj_frequency: 100e6,
            cdr_corner: 4e6,
        }
    }
}

impl JitterConfig {
    /// True if no jitter is configured.
    pub fn is_zero(&self) -> bool {
        self.rj_rms == 0.0 && self.dcd == 0.0 && self.sj_amplitude == 0.0
    }
}

impl Default for TxConfig {
    fn default() -> Self {
        Self {
            baud: 53.125e9,
            sps: 16,
            pattern_periods: 1,
            prbs_seed: crate::prbs::PRBS13_DEFAULT_SEED,
            p_avg_dbm: 0.0,
            er_db: 4.0,
            levels: 4,
            jitter: JitterConfig::default(),
        }
    }
}

/// VCSEL source model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VcselConfig {
    /// Center wavelength \[m\].
    pub wavelength: f64,
    /// RMS spectral width \[m\].
    pub spectral_width_rms: f64,
    /// Relaxation-oscillation (resonance) frequency \[Hz\].
    pub resonance_freq: f64,
    /// Damping ratio ζ of the 2nd-order response (dimensionless).
    pub damping: f64,
    /// Relative intensity noise \[dB/Hz\].
    pub rin_db_hz: f64,
}

impl Default for VcselConfig {
    fn default() -> Self {
        Self {
            wavelength: 850e-9,
            spectral_width_rms: 0.4e-9,
            resonance_freq: 28e9,
            damping: 0.7,
            rin_db_hz: -140.0,
        }
    }
}

/// Launch / connector geometry at the fiber input facet.
///
/// The VCSEL near field is modeled as an incoherent, equal-power mixture of its lowest
/// `vcsel_mode_groups` Laguerre–Gauss transverse mode groups (group q contains the LG_pl with
/// 2p + |l| = q − 1). `vcsel_mode_groups = 1` is a single-mode Gaussian beam.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchConfig {
    /// Gaussian waist w of the VCSEL mode basis at the facet (1/e² intensity radius of LG00) \[m\].
    pub beam_radius: f64,
    /// Number of excited VCSEL transverse mode groups (≥ 1).
    pub vcsel_mode_groups: u32,
    /// Radial offset of the beam relative to the fiber axis (along x) \[m\].
    pub offset: f64,
    /// Tilt angle of the beam in the x–z plane, measured in air \[rad\].
    pub tilt: f64,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            beam_radius: 4.0e-6,
            vcsel_mode_groups: 4,
            offset: 0.0,
            tilt: 0.0,
        }
    }
}

/// Refractive-index profile shape.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Profile {
    /// Power-law graded index n²(r) = n₁²(1 − 2Δ (r/a)^α) in the core.
    Graded { alpha: f64 },
    /// Step index.
    Step,
}

/// Core glass and how the index profile changes with wavelength.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreMaterial {
    /// GeO₂-doped silica core, fused-silica cladding (real OM3/OM4/OM5 fibers). The NA fixes
    /// the GeO₂ fraction at the core centre at 850 nm; the core and cladding indices follow
    /// their own Sellmeier equations, so Δ and the NA change with wavelength (profile
    /// dispersion). `eq:material-germania`
    #[default]
    GermaniaDoped,
    /// Both indices follow fused silica with Δ fixed from the NA at the source wavelength
    /// (no profile dispersion). The model of versions ≤ 0.11.
    FixedDelta,
}

/// Reference wavelength of `attenuation_db_per_km` and of the photodiode responsivity \[m\].
pub const REFERENCE_WAVELENGTH: f64 = 850e-9;

/// Share of the 850 nm attenuation that scales like Rayleigh scattering, λ⁻⁴
/// (`eq:fiber-attenuation-spectrum`). Fixed by the OM5 cabled limits of TIA-568.3-D,
/// 3.0 dB/km at 850 nm and 2.3 dB/km at 953 nm; it then predicts 1.44 dB/km at 1300 nm
/// (limit 1.5).
pub const RAYLEIGH_FRACTION: f64 = 0.635_534_3;

/// How the fiber attenuation depends on wavelength.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttenuationSpectrum {
    /// α(λ) = α₈₅₀·[1 + f·((850 nm/λ)⁴ − 1)]: Rayleigh scattering (share f) plus a
    /// wavelength-independent part (`eq:fiber-attenuation-spectrum`).
    #[default]
    Rayleigh,
    /// The same attenuation at every wavelength (versions ≤ 0.15).
    Flat,
}

impl FiberConfig {
    /// Attenuation \[dB/km\] at `wavelength` \[m\]; exactly `attenuation_db_per_km` at 850 nm.
    pub fn attenuation_at(&self, wavelength: f64) -> f64 {
        match self.attenuation_spectrum {
            AttenuationSpectrum::Flat => self.attenuation_db_per_km,
            AttenuationSpectrum::Rayleigh => {
                let r = (REFERENCE_WAVELENGTH / wavelength).powi(4);
                self.attenuation_db_per_km * (1.0 + RAYLEIGH_FRACTION * (r - 1.0))
            }
        }
    }
}

/// Multimode fiber.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FiberConfig {
    /// Fiber length \[m\].
    pub length: f64,
    /// Core radius \[m\].
    pub core_radius: f64,
    /// Numerical aperture: at 850 nm for a germania-doped core, at the source wavelength
    /// for `fixed_delta`.
    pub na: f64,
    pub profile: Profile,
    /// Core glass (dispersion model).
    pub core_material: CoreMaterial,
    /// Attenuation at 850 nm \[dB/km\] (all modes). At other wavelengths it follows
    /// `attenuation_spectrum`.
    pub attenuation_db_per_km: f64,
    /// Wavelength dependence of the attenuation.
    pub attenuation_spectrum: AttenuationSpectrum,
    /// Additional lumped loss, e.g. connectors \[dB\].
    pub extra_loss_db: f64,
    /// Radial grid points per core radius used by the mode solver.
    pub grid_points_per_radius: usize,
    /// Computational domain radius / core radius.
    pub domain_factor: f64,
}

/// Standard multimode fiber types (component `mmf`, docs/model/mmf.md).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MmfType {
    Om3,
    #[default]
    Om4,
    Om5,
}

/// Version of the preset calibration table. A saved `mmf` node means the preset values of
/// the version it was saved with; bump when a calibration changes.
pub const MMF_PRESET_VERSION: u32 = 1;

/// Maximum cabled attenuation of OM3/OM4/OM5 at 850 nm \[dB/km\] (TIA-568 / ISO 11801).
pub const MMF_MAX_ATTENUATION_DB_PER_KM: f64 = 3.5;

impl MmfType {
    pub const ALL: [MmfType; 3] = [MmfType::Om3, MmfType::Om4, MmfType::Om5];

    pub fn name(self) -> &'static str {
        match self {
            MmfType::Om3 => "OM3",
            MmfType::Om4 => "OM4",
            MmfType::Om5 => "OM5",
        }
    }

    /// Profile α of the minimum-spec preset (preset version 1). Calibrated with the default
    /// VCSEL launch so that the effective modal bandwidth at 850 nm sits at the type's
    /// minimum: OM3 2000, OM4 and OM5 4700 MHz·km (computed 2002, 4710, 4704). OM3 and OM4
    /// lie on the over-compensated side of the optimum (α ≈ 2.035), so their bandwidth falls
    /// at longer wavelengths, as in published OM4 data; OM5 lies on the under-compensated
    /// side and keeps its bandwidth up to 953 nm (EMB there > 10 000 MHz·km, minimum 2470).
    pub fn alpha(self) -> f64 {
        match self {
            MmfType::Om3 => 2.1231,
            MmfType::Om4 => 2.0777,
            MmfType::Om5 => 2.0109,
        }
    }

    /// The preset's full fiber configuration (length 100 m, cabled attenuation 3.5 dB/km).
    pub fn config(self) -> FiberConfig {
        FiberConfig {
            length: 100.0,
            core_radius: 25e-6,
            na: 0.20,
            profile: Profile::Graded {
                alpha: self.alpha(),
            },
            core_material: CoreMaterial::GermaniaDoped,
            attenuation_db_per_km: MMF_MAX_ATTENUATION_DB_PER_KM,
            attenuation_spectrum: AttenuationSpectrum::Rayleigh,
            extra_loss_db: 0.0,
            grid_points_per_radius: 600,
            domain_factor: 2.0,
        }
    }

    /// The preset whose physics (everything except length, attenuation and extra loss)
    /// equals `f`, if any.
    pub fn matching(f: &FiberConfig) -> Option<MmfType> {
        MmfType::ALL.into_iter().find(|t| {
            let p = t.config();
            p.core_radius == f.core_radius
                && p.na == f.na
                && p.profile == f.profile
                && p.core_material == f.core_material
                && p.attenuation_spectrum == f.attenuation_spectrum
                && p.grid_points_per_radius == f.grid_points_per_radius
                && p.domain_factor == f.domain_factor
        })
    }
}

/// Profile α of the default fiber: the OM4 preset.
pub const OM4_DEFAULT_ALPHA: f64 = 2.0777;

/// The default fiber is the OM4 preset (minimum spec, 100 m).
impl Default for FiberConfig {
    fn default() -> Self {
        MmfType::Om4.config()
    }
}

/// PIN photodiode + TIA receiver.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RxConfig {
    /// Responsivity \[A/W\].
    pub responsivity: f64,
    /// Detector radius \[m\]; `None` collects all guided power.
    pub detector_radius: Option<f64>,
    /// TIA input-referred noise current density \[A/√Hz\].
    pub noise_density: f64,
    /// Transimpedance \[Ω\].
    pub transimpedance: f64,
    /// −3 dB frequency of the Bessel–Thomson filter relative to the baud rate.
    pub bt_f3db_rel: f64,
    /// Enable the receive filter (disable only for validation).
    pub filter_enabled: bool,
    pub thermal_noise: bool,
    pub shot_noise: bool,
    pub rin_noise: bool,
    /// Taps of the receiver FFE after the receive filter (T-spaced, adapted by constrained
    /// MMSE, `docs/model/ffe.md`); 0 = no equalizer. Default 5, as in the 802.3 reference
    /// receiver for TDECQ.
    pub ffe_taps: usize,
}

impl Default for RxConfig {
    fn default() -> Self {
        Self {
            responsivity: 0.5,
            detector_radius: None,
            noise_density: 15e-12,
            transimpedance: 1000.0,
            bt_f3db_rel: 0.5,
            filter_enabled: true,
            thermal_noise: true,
            shot_noise: true,
            rin_noise: true,
            ffe_taps: 5,
        }
    }
}

/// Complete link configuration.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinkConfig {
    pub tx: TxConfig,
    pub vcsel: VcselConfig,
    pub launch: LaunchConfig,
    pub fiber: FiberConfig,
    pub rx: RxConfig,
    /// Seed for all noise sources.
    pub seed: u64,
}
