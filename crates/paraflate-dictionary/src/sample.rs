use paraflate_core::ExecutionPolicy;

#[derive(Clone, Debug)]
pub struct SamplePlan {
    pub max_bytes_per_entry: usize,
    pub stride: usize,
}

impl SamplePlan {
    pub fn from_policy(policy: &ExecutionPolicy) -> Self {
        let max_bytes = policy
            .max_sample_bytes_per_entry
            .max(policy.min_sample_bytes);
        let stride = ((1.0 / policy.sample_rate.max(0.0001)) as usize).max(1);
        Self {
            max_bytes_per_entry: max_bytes,
            stride,
        }
    }

    pub fn window_for_entry(&self, uncompressed: u64) -> usize {
        let cap = self.max_bytes_per_entry;
        if uncompressed == 0 {
            return 0;
        }
        cap.min(uncompressed as usize)
    }
}
