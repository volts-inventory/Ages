//! Named figures + species-modality-grounded name grammar (the
//! M3 thread of M4's bounded-language system).
//!
//! Named figures are the only individuals modeled in the
//! sim. A founding civ ships with a 2–3 figure starting band; the
//! hard ceiling is 200 active discoverers per civ, scaled in M4 by
//! population × literacy × institution count. M3 lands the data
//! shape and the deterministic naming pipeline; the dynamic-cap
//! growth waits on the population-tech multipliers.
//!
//! **Naming reflects species sensorium** (project goal: different
//! worlds, different sciences). The `NameGrammar` picks one of five
//! strategies based on the species' dominant communication modality
//! — acoustic species get CV-syllable names ("Kotami"), visual
//! species get brightness-pattern names ("Brifladim"), chemical
//! species get compound-morpheme names ("Acetiodform"), tactile
//! species get rhythm names ("Tapscravib"), gestural species get
//! motion-sequence names ("Wavtwstcrl"). All strategies emit ASCII
//! so the event log stays portable; M4's bounded-language work
//! refines this further.

use crate::discovery::Hypothesizer;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use sim_arith::Real;
use sim_species::ModalityKind;

/// A named individual within a civ. `id` is stable for the life of
/// the run; `name` is generated at birth from the civ's name
/// grammar. `retired_tick` is `None` while the figure is active and
/// fills in when the figure dies / civ collapses (M4).
///
/// Each figure owns their own `Hypothesizer` (PR C alignment) and
/// observes only the cells where `cell_idx % n_active_figures ==
/// cell_assignment`. Two figures with different assignments see
/// different sample distributions and may confirm different
/// relations — this is real per-figure attribution, not the M3
/// round-robin label. M4 settlements replace the modulo split with
/// real geographic regions per figure.
#[derive(Debug, Clone)]
pub struct NamedFigure {
    pub id: u32,
    pub name: String,
    pub born_tick: u64,
    pub retired_tick: Option<u64>,
    /// Cell-subset assignment: this figure observes cells where
    /// `cell_idx % n_active_figures == cell_assignment`. Stable for
    /// the figure's life; reassignment on band growth waits on M4.
    pub cell_assignment: u32,
    /// Per-figure quantitative discovery pipeline. Each
    /// figure accumulates their own samples and confirms their own
    /// relations; the events they emit carry their `figure_id` as
    /// real attribution (replacing PR-#3's round-robin labelling).
    pub hypothesizer: Hypothesizer,
    /// Charisma scalar `[0, 1]`. Scales cosmology drift
    /// magnitudes; charismatic-founder trigger fires at
    /// `>= 0.8`.
    pub charisma: Real,
    /// curiosity scalar `[0, 1]`. Scales fit-attempt cadence
    /// in the hypothesizer once focus-priority wiring lands;
    /// sampled here for forward-compatibility.
    pub curiosity: Real,
    /// doubt scalar `[0, 1]`. Higher doubt → the figure is
    /// more willing to challenge a confirmed relation and trigger
    /// a refinement attempt. Wired as a multiplier on the
    /// refinement-readiness streak so high-doubt figures revise
    /// theories sooner.
    pub doubt: Real,
    /// Communicativeness scalar `[0, 1]`. Boosts the
    /// comprehension score on knowledge transmissions originating
    /// from this figure (inheritance + diffusion). High-
    /// communicativeness founders pass on more of their canon to
    /// successors.
    pub communicativeness: Real,
}

/// Communication-strategy classification used to pick a name pool.
/// Each strategy maps to a distinct atom inventory so names from
/// different-sensorium species are recognisably different.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameStrategy {
    /// Voiced sound — CV syllables ("Kotami").
    Acoustic,
    /// Light/brightness signals — flash-pattern morphemes
    /// ("Brifladim").
    Visual,
    /// Chemical compounds — "Acetiodform".
    Chemical,
    /// Surface contact patterns — "Tapscravib".
    Tactile,
    /// Body motion — "Wavtwstcrl".
    Gestural,
}

impl NameStrategy {
    /// Per-strategy atom pool — short ASCII morphemes the grammar
    /// samples a per-civ subset from.
    pub const fn pool(self) -> &'static [&'static str] {
        match self {
            NameStrategy::Acoustic => &[
                "ka", "ko", "ki", "ta", "to", "ti", "ma", "mo", "mi", "sa", "so", "si", "na", "no",
                "lu", "ro", "ze", "be",
            ],
            NameStrategy::Visual => &[
                "bri", "dim", "fla", "lux", "umb", "ire", "vex", "lit", "dar", "glo", "spk", "lum",
                "shd", "ray",
            ],
            NameStrategy::Chemical => &[
                "acet", "iod", "amin", "form", "etyl", "ester", "lact", "phen", "ket", "alk",
                "thio", "azo",
            ],
            NameStrategy::Tactile => &[
                "tap", "rub", "scra", "ridg", "vibr", "puls", "thrum", "knurl", "edge", "grip",
                "shr",
            ],
            NameStrategy::Gestural => &[
                "wav", "swp", "twst", "snap", "crl", "uplt", "dwn", "arc", "circ", "zag", "pivt",
                "hov",
            ],
        }
    }

    /// Pick a strategy from a species' modality vector. Priority
    /// order acoustic → visual → chemical → tactile → gestural
    /// reflects which channel a species' communication "speech" is
    /// most likely centred on. A species that lacks all five
    /// communication-suited modalities falls back to gestural
    /// (postural movement is universal).
    pub fn from_modalities(modalities: &[ModalityKind]) -> Self {
        let has = |m: ModalityKind| modalities.contains(&m);
        if has(ModalityKind::AcousticAir) || has(ModalityKind::AcousticWater) {
            return NameStrategy::Acoustic;
        }
        if has(ModalityKind::VisualLight)
            || has(ModalityKind::VisualPolarization)
            || has(ModalityKind::Bioluminescent)
        {
            return NameStrategy::Visual;
        }
        if has(ModalityKind::ChemicalPheromone) || has(ModalityKind::ChemicalTaste) {
            return NameStrategy::Chemical;
        }
        if has(ModalityKind::Tactile) || has(ModalityKind::Seismic) {
            return NameStrategy::Tactile;
        }
        NameStrategy::Gestural
    }
}

/// Per-civ name grammar. Holds a strategy chosen from the
/// species' modality vector plus a sampled subset of the
/// strategy's atom pool. Stable for the civ's lifetime; an
/// inheritor civ's grammar derives from this with optional drift
/// (M4).
#[derive(Debug, Clone)]
pub struct NameGrammar {
    pub strategy: NameStrategy,
    pub atoms: Vec<&'static str>,
}

impl NameGrammar {
    /// Derive a grammar deterministically from species modalities +
    /// `(civ_id, species_seed)`. Same inputs → identical inventory.
    pub fn derive(modalities: &[ModalityKind], civ_id: u32, species_seed: u64) -> Self {
        let strategy = NameStrategy::from_modalities(modalities);
        let mut rng = ChaCha20Rng::seed_from_u64(
            species_seed ^ (u64::from(civ_id) << 32) ^ 0xC1AC_1AC1_AC1A_C1AC,
        );
        let pool = strategy.pool();
        let n_atoms = rng.gen_range(4..=8).min(pool.len());
        let mut atoms: Vec<&'static str> = pool.to_vec();
        atoms.shuffle(&mut rng);
        atoms.truncate(n_atoms);
        atoms.sort_unstable();
        Self { strategy, atoms }
    }

    /// Generate a figure-name of `n_units` atom concatenations.
    /// Caller owns the RNG so multiple names can be drawn in a
    /// single deterministic stream (the founding band shares one).
    pub fn name(&self, rng: &mut ChaCha20Rng, n_units: usize) -> String {
        let mut out = String::new();
        for _ in 0..n_units {
            out.push_str(self.atoms[rng.gen_range(0..self.atoms.len())]);
        }
        if let Some(first) = out.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        out
    }
}

/// Construct the founding band for a civ : 2 or 3 figures with
/// names drawn from `grammar`, ids assigned sequentially starting at
/// `next_id`, monotonic `cell_assignment` (`0..band_size`), and a
/// fresh per-figure `Hypothesizer` seeded with `intelligence` and
/// `perceivable_template_ids`. Returns the figures plus the next
/// available id so the caller can keep the counter monotonic.
#[allow(clippy::too_many_arguments)]
pub fn found_band(
    grammar: &NameGrammar,
    civ_id: u32,
    species_seed: u64,
    founded_tick: u64,
    next_id: u32,
    intelligence: Real,
    perceivable_template_ids: &[u32],
    attempt_period: u64,
) -> (Vec<NamedFigure>, u32) {
    let mut rng = ChaCha20Rng::seed_from_u64(
        species_seed ^ (u64::from(civ_id) << 16) ^ founded_tick ^ 0xF16D_F16D_F16D_F16D,
    );
    let band_size = rng.gen_range(2..=3);
    let mut figures = Vec::with_capacity(band_size);
    let mut id = next_id;
    for i in 0..band_size {
        let n_units = rng.gen_range(2..=3);
        let name = grammar.name(&mut rng, n_units);
        // personality scalars sampled uniform [0, 1]
        // deterministically per figure. Each captures one axis
        // along which figures vary at the cognitive / social
        // level: charisma, curiosity, doubt
        // (drives refinement aggressiveness), communicativeness
        // (drives transmission comprehension).
        let charisma = Real::from_ratio(i64::from(rng.gen_range(0..=1000_i32)), 1000);
        let curiosity = Real::from_ratio(i64::from(rng.gen_range(0..=1000_i32)), 1000);
        let doubt = Real::from_ratio(i64::from(rng.gen_range(0..=1000_i32)), 1000);
        let communicativeness = Real::from_ratio(i64::from(rng.gen_range(0..=1000_i32)), 1000);
        figures.push(NamedFigure {
            id,
            name,
            born_tick: founded_tick,
            retired_tick: None,
            cell_assignment: u32::try_from(i).unwrap_or(0),
            hypothesizer: Hypothesizer::with_attempt_period(
                intelligence,
                perceivable_template_ids,
                attempt_period,
            ),
            charisma,
            curiosity,
            doubt,
            communicativeness,
        });
        id = id.saturating_add(1);
    }
    (figures, id)
}

/// Map a relation to a figure deterministically. Uses
/// `relation_id` modulo the active-figure count so each relation
/// stably attributes to the same figure across ticks. Returns `0`
/// (the M2-era cohort placeholder) if no figures are active.
pub fn attribute(figures: &[NamedFigure], relation_id: u32) -> u32 {
    let active: Vec<&NamedFigure> = figures
        .iter()
        .filter(|f| f.retired_tick.is_none())
        .collect();
    if active.is_empty() {
        return 0;
    }
    let idx = (relation_id as usize) % active.len();
    active[idx].id
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn strategy_picks_acoustic_when_species_has_acoustic_air() {
        let mods = vec![ModalityKind::Tactile, ModalityKind::AcousticAir];
        assert_eq!(NameStrategy::from_modalities(&mods), NameStrategy::Acoustic);
    }

    #[test]
    fn strategy_picks_visual_when_no_acoustic_but_has_visual() {
        let mods = vec![ModalityKind::VisualLight, ModalityKind::Tactile];
        assert_eq!(NameStrategy::from_modalities(&mods), NameStrategy::Visual);
    }

    #[test]
    fn strategy_picks_chemical_when_no_acoustic_visual() {
        let mods = vec![ModalityKind::ChemicalPheromone, ModalityKind::Tactile];
        assert_eq!(NameStrategy::from_modalities(&mods), NameStrategy::Chemical);
    }

    #[test]
    fn strategy_picks_tactile_when_only_contact_senses() {
        let mods = vec![ModalityKind::Tactile];
        assert_eq!(NameStrategy::from_modalities(&mods), NameStrategy::Tactile);
    }

    #[test]
    fn strategy_falls_back_to_gestural_with_no_comm_channels() {
        let mods = vec![ModalityKind::MagneticSense];
        assert_eq!(NameStrategy::from_modalities(&mods), NameStrategy::Gestural);
    }

    #[test]
    fn grammar_derivation_is_deterministic() {
        let mods = vec![ModalityKind::AcousticAir];
        let a = NameGrammar::derive(&mods, 1, 42);
        let b = NameGrammar::derive(&mods, 1, 42);
        assert_eq!(a.strategy, b.strategy);
        assert_eq!(a.atoms, b.atoms);
    }

    #[test]
    fn different_species_modalities_yield_different_strategies() {
        let g_aco = NameGrammar::derive(&[ModalityKind::AcousticAir], 1, 42);
        let g_vis = NameGrammar::derive(&[ModalityKind::VisualLight], 1, 42);
        assert_ne!(g_aco.strategy, g_vis.strategy);
        // Atom pools differ → atoms differ.
        let a: BTreeSet<&str> = g_aco.atoms.iter().copied().collect();
        let v: BTreeSet<&str> = g_vis.atoms.iter().copied().collect();
        assert!(a.is_disjoint(&v));
    }

    #[test]
    fn grammar_atoms_within_strategy_pool() {
        for strategy in [
            NameStrategy::Acoustic,
            NameStrategy::Visual,
            NameStrategy::Chemical,
            NameStrategy::Tactile,
            NameStrategy::Gestural,
        ] {
            let pool: BTreeSet<&str> = strategy.pool().iter().copied().collect();
            for seed in 0..16u64 {
                let g = NameGrammar::derive(&match_kind(strategy), 1, seed);
                if g.strategy != strategy {
                    continue;
                }
                for a in &g.atoms {
                    assert!(pool.contains(a), "atom {a:?} not in {strategy:?} pool");
                }
            }
        }
    }

    fn match_kind(strategy: NameStrategy) -> Vec<ModalityKind> {
        match strategy {
            NameStrategy::Acoustic => vec![ModalityKind::AcousticAir],
            NameStrategy::Visual => vec![ModalityKind::VisualLight],
            NameStrategy::Chemical => vec![ModalityKind::ChemicalPheromone],
            NameStrategy::Tactile => vec![ModalityKind::Tactile],
            NameStrategy::Gestural => vec![ModalityKind::MagneticSense],
        }
    }

    #[test]
    fn name_uses_only_grammar_atoms() {
        let g = NameGrammar::derive(&[ModalityKind::AcousticAir], 7, 99);
        let mut rng = ChaCha20Rng::seed_from_u64(0);
        let name = g.name(&mut rng, 3).to_ascii_lowercase();
        // The name must be the concatenation of atoms from g.atoms;
        // confirm by greedy-stripping.
        let mut remaining = name.as_str();
        while !remaining.is_empty() {
            let matched = g.atoms.iter().find(|a| remaining.starts_with(*a)).copied();
            let a = matched.unwrap_or_else(|| panic!("no atom prefix in {remaining:?}"));
            remaining = &remaining[a.len()..];
        }
    }

    fn test_figure(id: u32, retired_tick: Option<u64>) -> NamedFigure {
        NamedFigure {
            id,
            name: format!("F{id}"),
            born_tick: 0,
            retired_tick,
            cell_assignment: 0,
            hypothesizer: Hypothesizer::new(Real::ONE, &[]),
            charisma: Real::from_ratio(5, 10),
            curiosity: Real::from_ratio(5, 10),
            doubt: Real::from_ratio(5, 10),
            communicativeness: Real::from_ratio(5, 10),
        }
    }

    #[test]
    fn founding_band_is_two_or_three_figures() {
        let g = NameGrammar::derive(&[ModalityKind::AcousticAir], 1, 42);
        let (figs, next) = found_band(&g, 1, 42, 0, 1, Real::ONE, &[], 20);
        assert!((2..=3).contains(&figs.len()));
        assert_eq!(next, 1 + u32::try_from(figs.len()).unwrap());
        for (i, f) in figs.iter().enumerate() {
            assert_eq!(f.id, 1 + u32::try_from(i).unwrap());
            assert!(!f.name.is_empty());
            assert!(f.retired_tick.is_none());
        }
    }

    #[test]
    fn founding_band_is_deterministic() {
        let g = NameGrammar::derive(&[ModalityKind::AcousticAir], 1, 42);
        let (a, _) = found_band(&g, 1, 42, 0, 1, Real::ONE, &[], 20);
        let (b, _) = found_band(&g, 1, 42, 0, 1, Real::ONE, &[], 20);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.id, y.id);
            assert_eq!(x.name, y.name);
        }
    }

    #[test]
    fn founding_band_assigns_distinct_cell_indices() {
        let g = NameGrammar::derive(&[ModalityKind::AcousticAir], 1, 42);
        let (figs, _) = found_band(&g, 1, 42, 0, 1, Real::ONE, &[], 20);
        let assignments: BTreeSet<u32> = figs.iter().map(|f| f.cell_assignment).collect();
        assert_eq!(
            assignments.len(),
            figs.len(),
            "each figure must observe a distinct cell-subset"
        );
    }

    #[test]
    fn attribute_round_robins_over_active_figures() {
        let figs = vec![test_figure(1, None), test_figure(2, None)];
        let mut seen = BTreeSet::new();
        for rid in 0..10 {
            seen.insert(attribute(&figs, rid));
        }
        assert!(seen.contains(&1));
        assert!(seen.contains(&2));
    }

    #[test]
    fn attribute_skips_retired_figures() {
        let figs = vec![test_figure(1, Some(100)), test_figure(2, None)];
        for rid in 0..10 {
            assert_eq!(attribute(&figs, rid), 2);
        }
    }

    #[test]
    fn attribute_with_no_active_returns_zero_placeholder() {
        let figs: Vec<NamedFigure> = vec![test_figure(1, Some(50))];
        assert_eq!(attribute(&figs, 0), 0);
    }
}
