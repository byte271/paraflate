use crate::ids::EntryId;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub enum PredictiveMode {
    #[default]
    Off,
    Standard,
    Aggressive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub enum VerificationMode {
    #[default]
    Off,
    AfterWrite,
    Strict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub enum PlanningAggression {
    #[default]
    Safe,
    Balanced,
    Aggressive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub enum PredictedDeflatePath {
    #[default]
    Auto,
    Dynamic,
    Fixed,
}

#[derive(Clone, Debug)]
pub struct PredictiveRuntimeConfig {
    pub mode: PredictiveMode,
    pub verification: VerificationMode,
    pub planning: PlanningAggression,
}

impl Default for PredictiveRuntimeConfig {
    fn default() -> Self {
        Self {
            mode: PredictiveMode::Off,
            verification: VerificationMode::Off,
            planning: PlanningAggression::Safe,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EntryCompressionPlan {
    pub entry_id: EntryId,
    pub entropy_bits: f64,
    pub repeat_density: f64,
    pub duplicate_proxy: f64,
    pub match_strength_proxy: f64,
    pub recommended_stored: bool,
    pub deflate_path: PredictedDeflatePath,
    pub target_block_bytes: u64,
    pub lz77_chain_mult: f64,
    pub use_global_huffman: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PredictiveArchivePlan {
    pub entries: Vec<EntryCompressionPlan>,
}

impl PredictiveArchivePlan {
    pub fn for_entry(&self, id: EntryId) -> Option<&EntryCompressionPlan> {
        self.entries
            .get(id.0 as usize)
            .filter(|plan| plan.entry_id == id)
            .or_else(|| self.entries.iter().find(|plan| plan.entry_id == id))
    }

    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{EntryCompressionPlan, PredictedDeflatePath, PredictiveArchivePlan};
    use crate::EntryId;

    #[test]
    fn for_entry_uses_dense_index_when_available() {
        let plan = PredictiveArchivePlan {
            entries: vec![
                sample_plan(EntryId(0), 64 * 1024),
                sample_plan(EntryId(1), 128 * 1024),
            ],
        };

        assert_eq!(
            plan.for_entry(EntryId(1)).map(|entry| entry.target_block_bytes),
            Some(128 * 1024)
        );
    }

    #[test]
    fn for_entry_falls_back_for_sparse_vectors() {
        let plan = PredictiveArchivePlan {
            entries: vec![sample_plan(EntryId(7), 512 * 1024)],
        };

        assert_eq!(
            plan.for_entry(EntryId(7)).map(|entry| entry.target_block_bytes),
            Some(512 * 1024)
        );
        assert!(plan.for_entry(EntryId(3)).is_none());
    }

    fn sample_plan(entry_id: EntryId, target_block_bytes: u64) -> EntryCompressionPlan {
        EntryCompressionPlan {
            entry_id,
            entropy_bits: 4.0,
            repeat_density: 0.1,
            duplicate_proxy: 0.0,
            match_strength_proxy: 0.0,
            recommended_stored: false,
            deflate_path: PredictedDeflatePath::Auto,
            target_block_bytes,
            lz77_chain_mult: 1.0,
            use_global_huffman: false,
        }
    }
}
