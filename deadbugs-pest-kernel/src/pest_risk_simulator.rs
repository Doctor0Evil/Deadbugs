use std::f64::consts::E;

/// Species-agnostic context for one site and pest class.
#[derive(Clone, Debug)]
pub struct PestContext {
    // Structural / environmental descriptors (normalized or scalar).
    pub structure_type: String,   // "home", "restaurant", "farm", etc.
    pub climate_band: String,     // "arid-hot", "temperate", etc.
    pub human_proximity: f64,     // 0–1, 1 = continuous occupancy by vulnerable people.
    pub animal_proximity: f64,    // 0–1, 1 = pets/livestock always present.
    pub food_availability: f64,   // 0–1, 1 = abundant exposed food.
    pub water_availability: f64,  // 0–1, 1 = constant moisture.
    pub harborage_quality: f64,   // 0–1, 1 = many cracks/voids/clutter.
}

/// Species-specific parameters loaded from a plugin.
#[derive(Clone, Debug)]
pub struct PestSpeciesModel {
    pub species_id: String,       // e.g., "bedbug.cimex_lectularius", "rodent.rattus".
    // Baseline biological parameters (per day).
    pub base_arrival_rate: f64,   // λ0, arrivals per day without exclusion.
    pub base_repro_rate: f64,     // r0, net reproductive rate per day.
    pub seasonality_amp: f64,     // 0–1 amplitude of seasonal modulation.
    pub seasonality_phase: f64,   // radians or fraction of year.
    // Damage & eco weights.
    pub damage_sensitivity: f64,  // maps abundance → damage risk.
    pub eco_sensitivity: f64,     // maps interventions → ecosystem disturbance.
    // Normalization anchors.
    pub abundance_hard_limit: f64,// N_hard, population where r_pest→1.
    pub damage_hard_limit: f64,   // D_hard, damage metric where r_damage→1.
    pub eco_hard_limit: f64,      // E_hard, eco disturbance metric where r_eco→1.
}

/// Abstract, non-toxic control methods (physical, mechanical, behavioral).
#[derive(Clone, Debug)]
pub struct ControlAction {
    pub method_id: String,      // e.g., "exclusion.seal_cracks", "trap.snap", "sanitation.deep_clean".
    pub intensity: f64,         // 0–1, normalized effort level.
    pub continuous: bool,       // if true, effect persists over horizon.
    // Simulator-side parameters; in practice sourced from shard evidence.
    pub arrival_reduction_frac: f64,   // fraction reduction in λ due to this action.
    pub repro_reduction_frac: f64,     // fraction reduction in r due to this action.
    pub damage_reduction_frac: f64,    // fraction reduction in damage per pest contact.
    pub eco_disturbance_score: f64,    // 0–1, higher = more non-target disturbance (e.g., lethal traps).
}

/// A full candidate plan: set of actions with timing.
#[derive(Clone, Debug)]
pub struct InterventionPlan {
    pub actions: Vec<ControlAction>,
    pub horizon_days: u32,
}

/// Simulated state over time for one plan.
#[derive(Clone, Debug)]
pub struct PestRiskState {
    pub times_days: Vec<u32>,
    pub abundance: Vec<f64>,        // N_t
    pub damage_metric: Vec<f64>,    // D_t
    pub eco_metric: Vec<f64>,       // E_t
    pub r_pest: Vec<f64>,           // 0–1
    pub r_damage: Vec<f64>,         // 0–1
    pub r_eco: Vec<f64>,            // 0–1
    pub residual_v: Vec<f64>,       // Lyapunov-like V_t
}

/// Simulation-level configuration (weights and hard limits).
#[derive(Clone, Debug)]
pub struct SimulationConfig {
    pub w_pest: f64,
    pub w_damage: f64,
    pub w_eco: f64,
    pub r_pest_max: f64,
    pub r_damage_max: f64,
    pub r_eco_max: f64,
}

/// Result + convenience summary for guards/UI.
#[derive(Clone, Debug)]
pub struct SimulationResult {
    pub state: PestRiskState,
    pub violated_hard_limit: bool,
}

/// Species plugin trait so bedbug/rodent/cockroach modules can supply parameters.
pub trait PestSpeciesPlugin {
    fn species_model(&self, ctx: &PestContext) -> PestSpeciesModel;
}

/// Simple sinusoidal seasonality modifier (0–1 scaling of base rates).
fn seasonality_factor(day: u32, amp: f64, phase: f64) -> f64 {
    if amp <= 0.0 {
        return 1.0;
    }
    let t = day as f64;
    let angle = 2.0 * std::f64::consts::PI * (t / 365.0) + phase;
    1.0 + amp * angle.cos() // in [1-amp, 1+amp]
}

/// Clamp helper.
fn clamp01(x: f64) -> f64 {
    if x <= 0.0 {
        0.0
    } else if x >= 1.0 {
        1.0
    } else {
        x
    }
}

/// Core simulator: discrete-time, non-actuating pest-pressure model.
pub fn simulate_pest_risk(
    ctx: &PestContext,
    species: &PestSpeciesModel,
    plan: &InterventionPlan,
    cfg: &SimulationConfig,
) -> SimulationResult {
    let horizon = plan.horizon_days.max(1);
    let mut times = Vec::with_capacity(horizon as usize + 1);
    let mut n = Vec::with_capacity(horizon as usize + 1);
    let mut d = Vec::with_capacity(horizon as usize + 1);
    let mut e = Vec::with_capacity(horizon as usize + 1);
    let mut r_pest = Vec::with_capacity(horizon as usize + 1);
    let mut r_damage = Vec::with_capacity(horizon as usize + 1);
    let mut r_eco = Vec::with_capacity(horizon as usize + 1);
    let mut v = Vec::with_capacity(horizon as usize + 1);

    // Initial conditions: low but non-zero abundance, zero accumulated damage & eco disturbance.
    let mut n_t = 1.0_f64;
    let mut d_t = 0.0_f64;
    let mut e_t = 0.0_f64;

    // Precompute aggregate control effects (for now, assume constant in time).
    let mut arrival_mult = 1.0_f64;
    let mut repro_mult = 1.0_f64;
    let mut damage_mult = 1.0_f64;
    let mut eco_base = 0.0_f64;

    for a in &plan.actions {
        // No banned classes here: upstream curation must exclude chemicals/pathogens/gene drives.
        let f = a.intensity.clamp(0.0, 1.0);
        arrival_mult *= 1.0 - f * a.arrival_reduction_frac.clamp(0.0, 1.0);
        repro_mult *= 1.0 - f * a.repro_reduction_frac.clamp(0.0, 1.0);
        damage_mult *= 1.0 - f * a.damage_reduction_frac.clamp(0.0, 1.0);
        eco_base += f * a.eco_disturbance_score.clamp(0.0, 1.0);
    }

    let mut violated_hard = false;

    for day in 0..=horizon {
        let idx = day as usize;
        times.push(day);

        // 1. Compute normalized risk coordinates.
        let r_p = (n_t / species.abundance_hard_limit.max(1.0)).min(1.0);
        let r_d = (d_t / species.damage_hard_limit.max(1.0)).min(1.0);
        let r_e = (e_t / species.eco_hard_limit.max(1.0)).min(1.0);

        let r_p = clamp01(r_p);
        let r_d = clamp01(r_d);
        let r_e = clamp01(r_e);

        let v_t = cfg.w_pest * r_p + cfg.w_damage * r_d + cfg.w_eco * r_e;

        n.push(n_t);
        d.push(d_t);
        e.push(e_t);
        r_pest.push(r_p);
        r_damage.push(r_d);
        r_eco.push(r_e);
        v.push(v_t);

        if r_p > cfg.r_pest_max || r_d > cfg.r_damage_max || r_e > cfg.r_eco_max {
            violated_hard = true;
        }

        if day == horizon {
            break;
        }

        // 2. Update dynamics (discrete-time, simplified).
        let season = seasonality_factor(day, species.seasonality_amp, species.seasonality_phase);
        let lambda_t = species.base_arrival_rate * arrival_mult * season
            * ctx.food_availability.clamp(0.0, 1.0)
            * ctx.harborage_quality.clamp(0.0, 1.0);

        let r_eff = species.base_repro_rate * repro_mult
            * ctx.water_availability.clamp(0.0, 1.0);

        // Discrete logistic-like update with bounded growth.
        let growth = r_eff * n_t * (1.0 - n_t / species.abundance_hard_limit.max(1.0));
        let n_next = (n_t + growth + lambda_t).max(0.0);

        // Damage accumulates from abundance weighted by human/asset proximity and mitigation.
        let damage_increment = n_t
            * species.damage_sensitivity
            * ctx.human_proximity.clamp(0.0, 1.0)
            * damage_mult;
        let d_next = d_t + damage_increment.max(0.0);

        // Eco disturbance accumulates from intrusive/lethal methods and non-target exposure.
        let eco_increment = eco_base
            * species.eco_sensitivity
            * (ctx.animal_proximity.clamp(0.0, 1.0) + ctx.human_proximity.clamp(0.0, 1.0)) / 2.0;
        let e_next = (e_t + eco_increment.max(0.0)).min(species.eco_hard_limit.max(1.0));

        n_t = n_next;
        d_t = d_next;
        e_t = e_next;
    }

    let state = PestRiskState {
        times_days: times,
        abundance: n,
        damage_metric: d,
        eco_metric: e,
        r_pest,
        r_damage,
        r_eco,
        residual_v: v,
    };

    SimulationResult {
        state,
        violated_hard_limit: violated_hard,
    }
}
