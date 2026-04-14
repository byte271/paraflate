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
        self.entries.iter().find(|e| e.entry_id == id)
    }

    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}
