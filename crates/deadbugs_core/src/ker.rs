#![forbid(unsafe_code)]

use crate::model::{
    ControlFamily, ControlMethod, EffectivenessBand, KerScore, OutcomeLog, RiskCoordinates,
};

/// Helper: clamp into [0,1].
fn clamp01(x: f64) -> f64 {
    if x < 0.0 {
        0.0
    } else if x > 1.0 {
        1.0
    } else {
        x
    }
}

/// Compute normalized risk coordinates from a set of outcome logs.
fn compute_risk_coordinates(logs: &[OutcomeLog], method: &ControlMethod) -> RiskCoordinates {
    if logs.is_empty() {
        return RiskCoordinates::default();
    }

    let n = logs.len() as f64;

    let mut pet_events = 0.0;
    let mut wildlife_events = 0.0;
    let mut human_injury_events = 0.0;
    let mut air_events = 0.0;

    for log in logs {
        if log.side_effects.pet_incident {
            pet_events += 1.0;
        }
        if log.side_effects.wildlife_incident {
            wildlife_events += 1.0;
        }
        if log.side_effects.human_injury {
            human_injury_events += 1.0;
        }
        if log.side_effects.air_quality_concern {
            air_events += 1.0;
        }
    }

    // Simple frequency-based normalization; can be refined with exposure denominators later.
    let r_pets = clamp01(pet_events / n);
    let r_wildlife = clamp01(wildlife_events / n);
    let r_human_injury = clamp01(human_injury_events / n);
    let r_air = clamp01(air_events / n);

    // Waste corridor: baseline from material flags, nudged by reported burden.
    let base_waste = if method.uses_disposable_electronics || method.generates_persistent_plastic {
        0.7
    } else {
        0.2
    };

    let mut waste_extra = 0.0;
    for log in logs {
        match log.side_effects.waste_burden.as_str() {
            "high" => waste_extra += 0.2,
            "moderate" => waste_extra += 0.1,
            _ => {}
        }
    }
    let r_waste = clamp01(base_waste + waste_extra / n);

    RiskCoordinates {
        r_pets,
        r_wildlife,
        r_waste,
        r_air,
        r_human_injury,
    }
}

/// Knowledge-factor K based on evidence quality.
/// This is a virtual-only approximation from logs; external trial data can be layered later.
fn compute_k(logs: &[OutcomeLog]) -> f64 {
    if logs.is_empty() {
        return 0.1; // anecdotal / untested
    }

    let n = logs.len() as f64;

    // Reward consistency of effectiveness across logs.
    let mut high = 0.0;
    let mut med = 0.0;
    let mut low = 0.0;
    for log in logs {
        match log.effectiveness {
            EffectivenessBand::High => high += 1.0,
            EffectivenessBand::Medium => med += 1.0,
            EffectivenessBand::Low => low += 1.0,
        }
    }

    let ph = high / n;
    let pm = med / n;
    let pl = low / n;

    // Simple entropy-like penalty: more mixed outcomes → lower K.
    let variability = (ph * (1.0 - ph)) + (pm * (1.0 - pm)) + (pl * (1.0 - pl));

    let base = if n >= 20.0 {
        0.9
    } else if n >= 5.0 {
        0.7
    } else {
        0.4
    };

    clamp01(base * (1.0 - 0.5 * variability))
}

/// Eco-impact E: reward exclusion, hygiene, and selective traps; penalize waste-heavy methods.
fn compute_e(logs: &[OutcomeLog], method: &ControlMethod) -> f64 {
    // Base by control family.
    let base = match method.family {
        ControlFamily::Exclusion => 0.95,
        ControlFamily::Sanitation => 0.93,
        ControlFamily::HabitatChange => 0.9,
        ControlFamily::PredatorSupport => 0.88,
        ControlFamily::MechanicalKill => 0.8,
        ControlFamily::LiveCapture => 0.78,
        ControlFamily::MonitoringOnly => 0.7,
    };

    // Penalize plastics / disposable electronics as in biopack work.
    let mut penalty = 0.0;
    if method.uses_disposable_electronics {
        penalty += 0.15;
    }
    if method.generates_persistent_plastic {
        penalty += 0.1;
    }

    // If effectiveness is systematically low, effective eco-gain is reduced.
    let n = logs.len() as f64;
    if n > 0.0 {
        let mut high_or_med = 0.0;
        for log in logs {
            if matches!(
                log.effectiveness,
                EffectivenessBand::High | EffectivenessBand::Medium
            ) {
                high_or_med += 1.0;
            }
        }
        let success_frac = high_or_med / n;
        // Blend base with success fraction.
        let eff_factor = 0.5 + 0.5 * success_frac;
        clamp01((base * eff_factor) - penalty)
    } else {
        clamp01(base - penalty)
    }
}

/// Aggregate R from risk coordinates with corridor weights.
fn compute_r(coords: &RiskCoordinates) -> f64 {
    // Emphasize pets, human injury, and wildlife as protected corridors.
    let w_pets = 0.3;
    let w_human = 0.3;
    let w_wildlife = 0.2;
    let w_waste = 0.1;
    let w_air = 0.1;

    clamp01(
        w_pets * coords.r_pets
            + w_human * coords.r_human_injury
            + w_wildlife * coords.r_wildlife
            + w_waste * coords.r_waste
            + w_air * coords.r_air,
    )
}

/// Main scoring function: returns K, E, R and hard-violation flag.
pub fn score_method(method: &ControlMethod, logs: &[OutcomeLog]) -> KerScore {
    let coords = compute_risk_coordinates(logs, method);
    let k = compute_k(logs);
    let e = compute_e(logs, method);
    let r = compute_r(&coords);

    // Hard invariants: any corridor at 1.0 on protected dimensions disallows the method.
    let hard_violation = coords.r_pets >= 1.0
        || coords.r_human_injury >= 1.0
        || coords.r_wildlife >= 1.0;

    KerScore {
        k,
        e,
        r,
        coords,
        hard_violation,
    }
}
