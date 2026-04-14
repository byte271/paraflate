use paraflate_core::{ParaflateError, ParaflateResult};

pub use paraflate_verify::{scan_end_of_central_directory, ArchiveStructuralSummary};

pub fn local_header_payload_bounds(
    data: &[u8],
    local_offset: u64,
    name_len: u16,
    extra_len: u16,
    compressed_size: u32,
) -> ParaflateResult<(u64, u64)> {
    let base = local_offset.saturating_add(30);
    let name_len = name_len as u64;
    let extra_len = extra_len as u64;
    let hdr_end = base.saturating_add(name_len).saturating_add(extra_len);
    let pay_end = hdr_end.saturating_add(compressed_size as u64);
    if pay_end > data.len() as u64 {
        return Err(ParaflateError::ArchiveConsistency(
            "local entry payload out of range".into(),
        ));
    }
    Ok((hdr_end, pay_end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eocd_detects_written_zip() {
        use paraflate_core::CompressionMethod;
        use paraflate_zip::{LocalHeaderSpec, ZipWriter};
        let mut v = Vec::new();
        {
            let mut z = ZipWriter::new(&mut v);
            let spec = LocalHeaderSpec {
                name: "t.txt".into(),
                method: CompressionMethod::Stored,
                crc32: 0,
                compressed_size: 1,
                uncompressed_size: 1,
                dos_time: 0,
                dos_date: 0,
            };
            z.write_local_entry(spec, b"q").expect("write");
            let (_w, _s) = z.finalize().expect("fin");
        }
        let s = scan_end_of_central_directory(&v).expect("eocd");
        assert!(s.total_entries >= 1);
    }

    #[test]
    fn eocd_missing_on_random_tail() {
        let junk = vec![0x5Au8; 64];
        assert!(scan_end_of_central_directory(&junk).is_err());
    }
}
