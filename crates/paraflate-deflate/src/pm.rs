use std::cmp;
use std::mem;
use std::ops;
use std::slice;

use itertools::Itertools;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PmError {
    NoSymbols,
    MaxLenTooSmall,
    MaxLenTooLarge,
}

fn order_non_nan<T: PartialOrd>(a: &T, b: &T) -> cmp::Ordering {
    a.partial_cmp(b).unwrap_or(cmp::Ordering::Equal)
}

fn complete_chunks<T>(mut slice: &[T], csize: usize) -> slice::Chunks<'_, T> {
    let remainder = slice.len() % csize;
    if remainder > 0 {
        slice = &slice[0..(slice.len() - remainder)];
    }
    slice.chunks(csize)
}

pub fn package_merge<Num>(frequencies: &[Num], max_len: u32) -> Result<Vec<u32>, PmError>
where
    Num: PartialOrd + Copy + ops::Add<Output = Num>,
{
    if frequencies.is_empty() {
        return Err(PmError::NoSymbols);
    }
    if frequencies.len() > (1usize << max_len) {
        return Err(PmError::MaxLenTooSmall);
    }
    if max_len > 32 {
        return Err(PmError::MaxLenTooLarge);
    }
    let sorted = {
        let mut tmp: Vec<_> = (0..frequencies.len()).collect();
        tmp.sort_by(|&a, &b| order_non_nan(&frequencies[a], &frequencies[b]));
        tmp
    };
    let capa = frequencies.len() * 2 - 1;
    let mut list: Vec<Num> = Vec::with_capacity(capa);
    let mut flags: Vec<u32> = vec![0; capa];
    let mut merged: Vec<Num> = Vec::with_capacity(capa);
    for depth in 0..max_len {
        merged.clear();
        let mask = 1u32 << depth;
        let pairs = complete_chunks(&list, 2).map(|s| (s[0] + s[1], true));
        let srted = sorted.iter().map(|&i| (frequencies[i], false));
        for (p, m) in pairs.merge_by(srted, |a, b| a.0 < b.0) {
            if m {
                flags[merged.len()] |= mask;
            }
            merged.push(p);
        }
        mem::swap(&mut merged, &mut list);
    }
    let mut n = frequencies.len() * 2 - 2;
    debug_assert!(list.len() >= n);
    let mut code_lens = vec![0u32; frequencies.len()];
    let mut depth = max_len;
    while depth > 0 && n > 0 {
        depth -= 1;
        let mask = 1u32 << depth;
        let mut merged = 0;
        for i in 0..n {
            if (flags[i] & mask) == 0 {
                code_lens[sorted[i - merged]] += 1;
            } else {
                merged += 1;
            }
        }
        n = merged * 2;
    }
    Ok(code_lens)
}

#[cfg(test)]
mod tests {
    use super::package_merge;

    #[test]
    fn pm_smoke() {
        let f = [1u64, 32, 16, 4, 8, 2, 1];
        let cl = package_merge(&f, 8).unwrap();
        assert_eq!(&cl[..], &[6, 1, 2, 4, 3, 5, 6]);
    }
}
