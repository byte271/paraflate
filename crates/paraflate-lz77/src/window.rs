#[derive(Clone, Copy)]
pub struct Window<'a> {
    pub data: &'a [u8],
    pub rel_base: u64,
}

impl<'a> Window<'a> {
    pub fn new(data: &'a [u8], rel_base: u64) -> Self {
        Self { data, rel_base }
    }

    pub fn abs_at(&self, i: usize) -> u64 {
        self.rel_base.saturating_add(i as u64)
    }
}
