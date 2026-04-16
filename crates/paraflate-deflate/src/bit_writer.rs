/// A bit-packing writer that accumulates bits in a 64-bit buffer and
/// flushes 8 bytes at a time, minimising per-byte branch overhead.
pub struct BitWriter {
    out: Vec<u8>,
    bit_buf: u64,
    bits_used: u8,
}

impl BitWriter {
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            out: Vec::with_capacity(cap),
            bit_buf: 0,
            bits_used: 0,
        }
    }

    /// Append `n` bits from `val` (LSB-first, as DEFLATE requires).
    #[inline(always)]
    pub fn put_bits(&mut self, val: u32, n: u8) {
        debug_assert!(n <= 32);
        if usize::from(self.bits_used) + usize::from(n) > 64 {
            self.flush_full_bytes();
        }
        let mask = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
        self.bit_buf |= ((val & mask) as u64) << self.bits_used;
        self.bits_used += n;
        self.flush_full_bytes();
    }

    pub fn finish(mut self) -> Vec<u8> {
        self.flush_full_bytes();
        if self.bits_used > 0 {
            self.out.push(self.bit_buf as u8);
        }
        self.out
    }

    #[inline(always)]
    fn flush_full_bytes(&mut self) {
        let flush_bytes = (self.bits_used / 8) as usize;
        if flush_bytes == 0 {
            return;
        }
        let bytes = self.bit_buf.to_le_bytes();
        self.out.extend_from_slice(&bytes[..flush_bytes]);
        if flush_bytes == 8 {
            self.bit_buf = 0;
        } else {
            self.bit_buf >>= flush_bytes * 8;
        }
        self.bits_used -= (flush_bytes as u8) * 8;
    }
}

#[cfg(test)]
mod tests {
    use super::BitWriter;

    fn reference_bits(ops: &[(u32, u8)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut bit_buf = 0u64;
        let mut bits_used = 0u8;
        for &(val, n) in ops {
            let mask = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
            bit_buf |= ((val & mask) as u64) << bits_used;
            bits_used += n;
            while bits_used >= 8 {
                out.push(bit_buf as u8);
                bit_buf >>= 8;
                bits_used -= 8;
            }
        }
        if bits_used > 0 {
            out.push(bit_buf as u8);
        }
        out
    }

    #[test]
    fn finish_flushes_all_pending_bytes() {
        let ops = [(0xABCD, 16)];
        let mut writer = BitWriter::with_capacity(16);
        for (val, bits) in ops {
            writer.put_bits(val, bits);
        }
        assert_eq!(writer.finish(), reference_bits(&ops));
    }

    #[test]
    fn handles_writes_that_cross_the_u64_boundary() {
        let ops = [
            (0x89AB_CDEF, 32),
            (0x0123_4567, 32),
            (0b101_0110, 7),
            (0x1357_9BDF, 32),
            (0b11, 2),
        ];
        let mut writer = BitWriter::with_capacity(64);
        for (val, bits) in ops {
            writer.put_bits(val, bits);
        }
        assert_eq!(writer.finish(), reference_bits(&ops));
    }
}
