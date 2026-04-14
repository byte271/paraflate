#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveLayout {
    DeterministicLexical,
    SizeDescending,
    GlobalScoreDescending,
}

#[derive(Clone, Debug)]
pub struct ArchiveProfile {
    pub layout: ArchiveLayout,
    pub compression: super::CompressionProfile,
    pub execution: super::ExecutionPolicy,
    pub budget: super::ExecutionBudget,
    pub reproducible: bool,
    pub predictive: super::predictive::PredictiveRuntimeConfig,
}

impl Default for ArchiveProfile {
    fn default() -> Self {
        Self {
            layout: ArchiveLayout::DeterministicLexical,
            compression: super::CompressionProfile::default(),
            execution: super::ExecutionPolicy::default(),
            budget: super::ExecutionBudget::default(),
            reproducible: true,
            predictive: super::predictive::PredictiveRuntimeConfig::default(),
        }
    }
}
