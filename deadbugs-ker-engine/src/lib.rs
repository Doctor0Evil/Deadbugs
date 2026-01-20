#[derive(Clone, Debug)]
pub struct ControlMethod {
    pub method_id: String,
    pub class: String,                  // "exclusion", "trap", "sanitation", "habitat_mod".
    pub lethal: bool,                   // prefer false for eco-impact.
    pub chemical: bool,                 // must be false in curated library.
}

#[derive(Clone, Debug)]
pub struct OutcomeLog {
    pub method_id: String,
    pub pest_class: String,
    pub context_hash: String,           // hex of PestContext shard.
    pub n_cases: u32,
    pub effectiveness_band: f64,        // 0–1 (field-logged, not guessed).
    pub bycatch_band: f64,              // 0–1 (0 = no bycatch).
    pub waste_band: f64,                // 0–1 (0 = minimal waste).
}

#[derive(Clone, Debug)]
pub struct RiskScoreKER {
    pub method_id: String,
    pub k_knowledge: f64,   // 0–1
    pub e_eco_impact: f64,  // 0–1 (higher = more eco-positive).
    pub r_risk_harm: f64,   // 0–1 (higher = more harmful).
}

/// Aggregate K/E/R from outcome logs for a single method.
pub fn score_method(method: &ControlMethod, logs: &[OutcomeLog]) -> RiskScoreKER {
    let mut total_cases: f64 = 0.0;
    let mut eff_weighted: f64 = 0.0;
    let mut bycatch_weighted: f64 = 0.0;
    let mut waste_weighted: f64 = 0.0;

    for log in logs.iter().filter(|l| l.method_id == method.method_id) {
        let w = log.n_cases.max(1) as f64;
        total_cases += w;
        eff_weighted += w * log.effectiveness_band.clamp(0.0, 1.0);
        bycatch_weighted += w * log.bycatch_band.clamp(0.0, 1.0);
        waste_weighted += w * log.waste_band.clamp(0.0, 1.0);
    }

    let (avg_eff, avg_bycatch, avg_waste) = if total_cases > 0.0 {
        (
            eff_weighted / total_cases,
            bycatch_weighted / total_cases,
            waste_weighted / total_cases,
        )
    } else {
        (0.0, 0.5, 0.5) // low evidence defaults.
    };

    // Knowledge: more and more consistent logs → higher K.
    let k = if total_cases == 0.0 {
        0.6 // generic engineering knowledge only.
    } else {
        // Saturating curve: 10+ cases → near 1.0 when effectiveness is moderate+.
        let evidence_factor = (1.0 - E.powf(-total_cases / 10.0)).clamp(0.0, 1.0);
        0.5 + 0.5 * evidence_factor * avg_eff
    };

    // Eco-impact: reward non-lethal, non-chemical, low bycatch & waste, and permanent exclusion.
    let lethal_penalty = if method.lethal { 0.3 } else { 0.0 };
    let chemical_penalty = if method.chemical { 0.6 } else { 0.0 };
    let class_bonus = match method.class.as_str() {
        "exclusion" | "sanitation" | "habitat_mod" => 0.3,
        _ => 0.1,
    };

    let mut e = 0.5
        + class_bonus
        + 0.2 * (1.0 - avg_bycatch)
        + 0.2 * (1.0 - avg_waste)
        - lethal_penalty
        - chemical_penalty;
    e = e.clamp(0.0, 1.0);

    // Risk-of-harm: dominated by bycatch, waste, and lethal/chemical flags.
    let mut r = 0.2 * avg_bycatch + 0.2 * avg_waste;
    if method.lethal {
        r += 0.3;
    }
    if method.chemical {
        r += 0.4;
    }
    r = r.clamp(0.0, 1.0);

    RiskScoreKER {
        method_id: method.method_id.clone(),
        k_knowledge: k,
        e_eco_impact: e,
        r_risk_harm: r,
    }
}
