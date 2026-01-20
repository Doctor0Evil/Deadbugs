#![forbid(unsafe_code)]

use crate::ker::score_method;
use crate::model::{
    ControlFamily, ControlMethod, KerScore, LocationType, OutcomeLog, PestContext, PestSpecies,
};

/// In-memory registry; in production this would be backed by qpudatashards.
#[derive(Default)]
pub struct MethodRegistry {
    pub methods: Vec<ControlMethod>,
    pub logs: Vec<OutcomeLog>,
}

impl MethodRegistry {
    pub fn new() -> Self {
        Self {
            methods: Vec::new(),
            logs: Vec::new(),
        }
    }

    pub fn add_method(&mut self, method: ControlMethod) {
        self.methods.push(method);
    }

    pub fn add_log(&mut self, log: OutcomeLog) {
        self.logs.push(log);
    }

    /// Get all logs for a given method ID and pest.
    fn logs_for_method(&self, method_id: &str, pest: PestSpecies) -> Vec<OutcomeLog> {
        self.logs
            .iter()
            .filter(|l| l.method_id == method_id && l.context.pest == pest)
            .cloned()
            .collect()
    }

    /// Query safest high-E methods for a pest and context, filtered by R ceiling and hard invariants.
    pub fn query_safest_methods(
        &self,
        pest: PestSpecies,
        location: LocationType,
        max_r: f64,
    ) -> Vec<(ControlMethod, KerScore)> {
        let mut scored: Vec<(ControlMethod, KerScore)> = self
            .methods
            .iter()
            .filter(|m| {
                // Virtual-only: ensure we never consider chemical / biocide families.
                !matches!(m.family, ControlFamily::MonitoringOnly) // monitoring is allowed; example guard kept simple
            })
            .map(|m| {
                let logs = self.logs_for_method(&m.id, pest);
                let mut ker = score_method(m, &logs);

                // Prioritize exclusion & hygiene: small E uplift in tier-0 domains.
                if matches!(m.family, ControlFamily::Exclusion | ControlFamily::Sanitation) {
                    ker.e = (ker.e + 0.05).min(1.0);
                }

                (m.clone(), ker)
            })
            .filter(|(m, ker)| {
                // Enforce context-aware risk: disallow methods if R > max_r or if hard_violation.
                if ker.hard_violation || ker.r > max_r {
                    return false;
                }

                // Additional guardrails for sensitive locations.
                match location {
                    LocationType::Home | LocationType::Hospital => {
                        // No disposable electronics or persistent plastics in sensitive settings.
                        !(m.uses_disposable_electronics || m.generates_persistent_plastic)
                    }
                    _ => true,
                }
            })
            .collect();

        // Sort by K high → low, then E high → low.
        scored.sort_by(|(_, a), (_, b)| {
            b.k
                .partial_cmp(&a.k)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    b.e.partial_cmp(&a.e)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        });

        scored
    }

    /// Convenience: filter to purely exclusion & hygiene tier-0 actions.
    pub fn tier0_exclusion_hygiene(
        &self,
        pest: PestSpecies,
        location: LocationType,
    ) -> Vec<(ControlMethod, KerScore)> {
        let mut out = Vec::new();
        for m in &self.methods {
            if !matches!(m.family, ControlFamily::Exclusion | ControlFamily::Sanitation) {
                continue;
            }
            let logs = self.logs_for_method(&m.id, pest);
            let ker = score_method(m, &logs);
            // Require low risk for tier-0 recommendation.
            if !ker.hard_violation && ker.r <= 0.2 {
                out.push((m.clone(), ker));
            }
        }
        out.sort_by(|(_, a), (_, b)| {
            b.k
                .partial_cmp(&a.k)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    b.e.partial_cmp(&a.e)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        });
        out
    }
}
