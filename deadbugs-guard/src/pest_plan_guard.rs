use deadbugs_pest_kernel::{PestRiskState, SimulationResult};

/// Guard configuration (pulled from DID-signed shards in production).
#[derive(Clone, Debug)]
pub struct PlanGuardConfig {
    pub v_max: f64,          // optional upper bound on residual V_t.
    pub require_v_nonincrease: bool,
    pub require_all_below_max: bool,
}

/// Verdict returned to API/UI.
#[derive(Clone, Debug)]
pub struct GuardVerdict {
    pub corridor_safe: bool,
    pub hard_limit_violated: bool,
    pub v_nonincreasing: bool,
    pub v_exceeded_max: bool,
}

/// Checks hard risk limits and V_t monotonicity.
pub fn evaluate_plan_guard(
    sim: &SimulationResult,
    cfg: &PlanGuardConfig,
) -> GuardVerdict {
    let state: &PestRiskState = &sim.state;
    let mut hard_violation = sim.violated_hard_limit;
    let mut v_noninc = true;
    let mut v_exceeded = false;

    let vt = &state.residual_v;
    for w in vt.windows(2) {
        let v_t = w[0];
        let v_next = w[1];
        if cfg.require_v_nonincrease && v_next > v_t + 1e-9 {
            v_noninc = false;
            break;
        }
    }

    if cfg.require_all_below_max {
        for &val in vt {
            if val > cfg.v_max {
                v_exceeded = true;
                break;
            }
        }
    }

    let corridor_safe = !hard_violation && v_noninc && !v_exceeded;

    GuardVerdict {
        corridor_safe,
        hard_limit_violated: hard_violation,
        v_nonincreasing: v_noninc,
        v_exceeded_max: v_exceeded,
    }
}
