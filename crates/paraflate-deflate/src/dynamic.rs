use crate::pm::{package_merge, PmError};

pub fn length_limited_lengths(freq: &[u64], max_len: u32) -> Result<Vec<u8>, PmError> {
    let v = package_merge(freq, max_len)?;
    Ok(v.into_iter().map(|x| x.min(max_len) as u8).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_limited_smoke() {
        let f = vec![1u64, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3];
        let l = length_limited_lengths(&f, 7).unwrap();
        assert_eq!(l.len(), f.len());
        assert!(l.iter().all(|&x| x <= 7));
    }
}
