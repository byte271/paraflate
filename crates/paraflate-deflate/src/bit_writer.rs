pub struct BitWriter {
    out: Vec<u8>,
    bit_buf: u64,
    bits_used: u8,
}

impl BitWriter {
    pub fn new() -> Self {
        Self {
            out: Vec::new(),
            bit_buf: 0,
            bits_used: 0,
        }
    }

    pub fn put_bits(&mut self, val: u32, n: u8) {
        debug_assert!(n <= 32);
        let mask = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
        let v = (val & mask) as u64;
        self.bit_buf |= v << self.bits_used;
        self.bits_used = self.bits_used.saturating_add(n);
        while self.bits_used >= 8 {
            self.out.push(self.bit_buf as u8);
            self.bit_buf >>= 8;
            self.bits_used -= 8;
        }
    }

    pub fn finish(mut self) -> Vec<u8> {
        if self.bits_used > 0 {
            self.out.push(self.bit_buf as u8);
        }
        self.out
    }
}
