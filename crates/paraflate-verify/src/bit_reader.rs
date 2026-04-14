pub struct BitReader<'a> {
    pub bytes: &'a [u8],
    pub pos: usize,
    pub bit_buf: u32,
    pub bit_cnt: u8,
}

impl<'a> BitReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: 0,
            bit_buf: 0,
            bit_cnt: 0,
        }
    }

    pub fn ensure(&mut self, n: u8) -> Result<(), ()> {
        while self.bit_cnt < n {
            let b = *self.bytes.get(self.pos).ok_or(())?;
            self.pos += 1;
            self.bit_buf |= (b as u32) << self.bit_cnt;
            self.bit_cnt += 8;
        }
        Ok(())
    }

    pub fn read_bits(&mut self, n: u8) -> Result<u32, ()> {
        if n == 0 {
            return Ok(0);
        }
        self.ensure(n)?;
        let mask = (1u32 << n) - 1;
        let v = self.bit_buf & mask;
        self.bit_buf >>= n;
        self.bit_cnt -= n;
        Ok(v)
    }

    pub fn align_to_byte_boundary(&mut self) -> Result<(), ()> {
        let waste = (8 - (self.bit_cnt % 8)) % 8;
        for _ in 0..waste {
            self.read_bits(1)?;
        }
        Ok(())
    }
}
