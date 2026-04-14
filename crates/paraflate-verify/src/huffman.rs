use crate::bit_reader::BitReader;

const NONE: u16 = u16::MAX;

#[derive(Clone, Debug)]
pub struct HuffNode {
    pub leaf_sym: Option<u16>,
    pub ch: [u16; 2],
}

impl HuffNode {
    pub fn empty() -> Self {
        Self {
            leaf_sym: None,
            ch: [NONE, NONE],
        }
    }
}

pub fn fixed_lit_lengths() -> [u8; 288] {
    let mut lit_len = [0u8; 288];
    for i in 0..144 {
        lit_len[i] = 8;
    }
    for i in 144..256 {
        lit_len[i] = 9;
    }
    for i in 256..280 {
        lit_len[i] = 7;
    }
    for i in 280..288 {
        lit_len[i] = 8;
    }
    lit_len
}

pub fn fixed_dist_lengths() -> [u8; 30] {
    [5u8; 30]
}

fn reverse_bits(mut code: u32, len: u8) -> u16 {
    let mut out = 0u32;
    let mut i = 0u8;
    while i < len {
        out = (out << 1) | (code & 1);
        code >>= 1;
        i += 1;
    }
    out as u16
}

pub fn canonical_pairs(lens: &[u8], max_sym: usize) -> Vec<(u16, u16, u8)> {
    let mut bl = [0u32; 16];
    let mut max_b = 0usize;
    for s in 0..max_sym {
        let l = lens[s] as usize;
        if l > 0 {
            bl[l] += 1;
            max_b = max_b.max(l);
        }
    }
    let mut next_code = [0u32; 16];
    let mut code = 0u32;
    for bits in 1..=max_b {
        code = (code + bl[bits - 1]) << 1;
        next_code[bits] = code;
    }
    let mut out = Vec::new();
    for s in 0..max_sym {
        let l = lens[s] as usize;
        if l > 0 {
            let c = next_code[l];
            next_code[l] += 1;
            let rev = reverse_bits(c, l as u8);
            out.push((s as u16, rev, l as u8));
        }
    }
    out
}

pub fn build_decode_tree(lens: &[u8], max_sym: usize) -> Result<Vec<HuffNode>, ()> {
    let pairs = canonical_pairs(lens, max_sym);
    let mut nodes = vec![HuffNode::empty()];
    for (sym, code, len) in pairs {
        insert_path(&mut nodes, sym, code, len)?;
    }
    Ok(nodes)
}

fn insert_path(nodes: &mut Vec<HuffNode>, sym: u16, code: u16, len: u8) -> Result<(), ()> {
    let mut idx = 0usize;
    for i in 0..len {
        let b = ((code as u32 >> i) & 1) as usize;
        if nodes[idx].leaf_sym.is_some() {
            return Err(());
        }
        let nxt = nodes[idx].ch[b];
        if nxt == NONE {
            nodes.push(HuffNode::empty());
            let ni = (nodes.len() - 1) as u16;
            nodes[idx].ch[b] = ni;
        }
        idx = nodes[idx].ch[b] as usize;
    }
    if nodes[idx].ch[0] != NONE || nodes[idx].ch[1] != NONE {
        return Err(());
    }
    if nodes[idx].leaf_sym.is_some() {
        return Err(());
    }
    nodes[idx].leaf_sym = Some(sym);
    Ok(())
}

pub fn decode_symbol(nodes: &[HuffNode], br: &mut BitReader) -> Result<u16, ()> {
    let mut idx = 0usize;
    loop {
        let bit = br.read_bits(1)? as usize;
        let nxt = nodes[idx].ch[bit];
        if nxt == NONE {
            return Err(());
        }
        let ni = nxt as usize;
        if let Some(s) = nodes[ni].leaf_sym {
            return Ok(s);
        }
        idx = ni;
    }
}
