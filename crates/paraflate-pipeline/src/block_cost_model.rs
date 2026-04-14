use paraflate_core::{CompressionMethod, PlanningAggression, PredictedDeflatePath, PredictiveMode};

pub fn byte_entropy_bits(data: &[u8]) -> f64 {
    if data.len() < 16 {
        return 0.0;
    }
    let take = data.len().min(262144);
    let mut c = [0u64; 256];
    let mut n = 0u64;
    for &b in &data[..take] {
        c[b as usize] += 1;
        n += 1;
    }
    if n == 0 {
        return 0.0;
    }
    let mut h = 0.0f64;
    for v in c {
        if v == 0 {
            continue;
        }
        let p = v as f64 / n as f64;
        h -= p * p.ln();
    }
    h / std::f64::consts::LN_2
}

pub fn repeat_density(data: &[u8]) -> f64 {
    if data.len() < 2 {
        return 0.0;
    }
    let take = data.len().min(262144);
    let mut same = 0u64;
    let mut tot = 0u64;
    for i in 1..take {
        if data[i] == data[i - 1] {
            same += 1;
        }
        tot += 1;
    }
    if tot == 0 {
        return 0.0;
    }
    same as f64 / tot as f64
}

pub fn bigram_repeat_proxy(data: &[u8]) -> f64 {
    if data.len() < 4 {
        return 0.0;
    }
    let take = data.len().min(131072);
    let mut hits = 0u64;
    let mut tot = 0u64;
    let mut i = 3usize;
    while i < take {
        if data[i] == data[i - 2] && data[i - 1] == data[i - 3] {
            hits += 1;
        }
        tot += 1;
        i += 1;
    }
    if tot == 0 {
        return 0.0;
    }
    hits as f64 / tot as f64
}

pub fn match_strength_proxy(entropy: f64, repeat: f64, bigram: f64) -> f64 {
    let e = entropy.clamp(0.0, 8.0);
    (repeat * 2.4 + bigram * 1.6 + (8.0 - e) * 0.35).max(0.0)
}

pub fn recommend_stored(
    uncompressed: u64,
    entropy: f64,
    match_proxy: f64,
    profile_method: CompressionMethod,
    planning: PlanningAggression,
    predictive: PredictiveMode,
) -> bool {
    if profile_method == CompressionMethod::Stored {
        return true;
    }
    if predictive == PredictiveMode::Off {
        return false;
    }
    if uncompressed == 0 {
        return true;
    }
    let ratio = match_proxy / (entropy + 0.25).max(0.25);
    let thr = match planning {
        PlanningAggression::Safe => 1.85,
        PlanningAggression::Balanced => 1.35,
        PlanningAggression::Aggressive => 0.95,
    };
    if uncompressed < 96 && entropy > 6.5 {
        return true;
    }
    if uncompressed < 2048 && entropy > 7.2 && ratio < thr * 0.6 {
        return true;
    }
    ratio < thr && entropy > 6.0
}

pub fn recommend_deflate_path(
    entropy: f64,
    match_proxy: f64,
    predictive: PredictiveMode,
    planning: PlanningAggression,
) -> PredictedDeflatePath {
    if predictive == PredictiveMode::Off {
        return PredictedDeflatePath::Auto;
    }
    let lift = match predictive {
        PredictiveMode::Aggressive => 0.35,
        PredictiveMode::Standard => 0.2,
        PredictiveMode::Off => 0.0,
    };
    let m = match_proxy + lift;
    let bias = match planning {
        PlanningAggression::Safe => 1.15,
        PlanningAggression::Balanced => 1.0,
        PlanningAggression::Aggressive => 0.85,
    };
    if m * bias > 2.8 - entropy * 0.12 {
        PredictedDeflatePath::Dynamic
    } else if entropy < 4.5 && m > 1.1 {
        PredictedDeflatePath::Fixed
    } else {
        PredictedDeflatePath::Auto
    }
}

pub fn target_block_bytes(
    uncompressed: u64,
    entropy: f64,
    repeat: f64,
    model_block: usize,
    policy_min: u64,
    policy_max: u64,
    predictive: PredictiveMode,
    planning: PlanningAggression,
) -> u64 {
    let mut b = model_block as u64;
    if predictive != PredictiveMode::Off {
        if repeat > 0.22 && entropy < 5.5 {
            b = b.saturating_mul(3).min(policy_max);
        } else if entropy > 7.2 {
            b = (b / 2).max(policy_min);
        }
        if predictive == PredictiveMode::Aggressive && repeat > 0.12 {
            b = b.saturating_mul(5).min(policy_max) / 4;
            b = b.max(policy_min);
        }
        if planning == PlanningAggression::Safe {
            b = b.min((policy_max / 2).max(policy_min));
        }
    }
    b.clamp(policy_min, policy_max).min(uncompressed.max(1))
}

pub fn lz77_chain_multiplier(
    match_proxy: f64,
    planning: PlanningAggression,
    predictive: PredictiveMode,
) -> f64 {
    let mut m = 1.0f64;
    if predictive == PredictiveMode::Aggressive {
        m *= 1.25;
    } else if predictive == PredictiveMode::Standard {
        m *= 1.08;
    }
    if planning == PlanningAggression::Aggressive {
        m *= 1.12;
    } else if planning == PlanningAggression::Safe {
        m *= 0.88;
    }
    m *= (0.85 + match_proxy * 0.08).clamp(0.75, 1.35);
    m.clamp(0.5, 1.75)
}

pub fn global_huffman_hint(
    uncompressed: u64,
    block_target: u64,
    repeat: f64,
    predictive: PredictiveMode,
) -> bool {
    if predictive == PredictiveMode::Off {
        return false;
    }
    let blocks = (uncompressed / block_target.max(1)).max(1);
    predictive != PredictiveMode::Off && blocks >= 2 && repeat > 0.08
}
