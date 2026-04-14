use crc32fast::Hasher;

use paraflate_core::{ParaflateError, ParaflateResult};

use crate::inflate::inflate_raw_stream;

const EOCD_SIG: u32 = 0x06054b50;
const LOCAL_SIG: u32 = 0x04034b50;
const CENTRAL_SIG: u32 = 0x02014b50;

#[derive(Clone, Debug)]
pub struct ArchiveStructuralSummary {
    pub eocd_offset: u64,
    pub disk_entries: u16,
    pub total_entries: u16,
    pub central_size: u32,
    pub central_offset: u32,
}

#[derive(Clone, Debug)]
pub struct VerifiedEntry {
    pub name: String,
    pub crc32: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub method: u16,
}

#[derive(Clone, Debug)]
pub struct VerificationReport {
    pub entries_verified: u64,
    pub inflated_bytes: u64,
    pub structure: ArchiveStructuralSummary,
    pub entries: Vec<VerifiedEntry>,
}

struct CdEntry {
    name: String,
    local_offset: u64,
    crc32: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    method: u16,
}

fn read_u16(data: &[u8], i: usize) -> ParaflateResult<u16> {
    let s = data
        .get(i..i + 2)
        .ok_or_else(|| ParaflateError::ZipStructure("u16".into()))?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn read_u32(data: &[u8], i: usize) -> ParaflateResult<u32> {
    let s = data
        .get(i..i + 4)
        .ok_or_else(|| ParaflateError::ZipStructure("u32".into()))?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

pub fn scan_end_of_central_directory(data: &[u8]) -> ParaflateResult<ArchiveStructuralSummary> {
    if data.len() < 22 {
        return Err(ParaflateError::ZipStructure(
            "archive shorter than EOCD".into(),
        ));
    }
    let window = 65557usize.min(data.len()).saturating_sub(22);
    let start = data.len().saturating_sub(22 + window);
    let mut pos = data.len().saturating_sub(22);
    while pos >= start {
        let sig = read_u32(data, pos)?;
        if sig == EOCD_SIG {
            let disk_entries = read_u16(data, pos + 8)?;
            let total_entries = read_u16(data, pos + 10)?;
            let central_size = read_u32(data, pos + 12)?;
            let central_offset = read_u32(data, pos + 16)?;
            let cend = central_offset as u64 + central_size as u64;
            if cend > pos as u64 {
                return Err(ParaflateError::ArchiveConsistency(
                    "central directory extends past EOCD".into(),
                ));
            }
            return Ok(ArchiveStructuralSummary {
                eocd_offset: pos as u64,
                disk_entries,
                total_entries,
                central_size,
                central_offset,
            });
        }
        if pos == 0 {
            break;
        }
        pos -= 1;
    }
    Err(ParaflateError::ZipStructure(
        "end of central directory signature not found".into(),
    ))
}

fn parse_central_directory(
    data: &[u8],
    structure: &ArchiveStructuralSummary,
) -> ParaflateResult<Vec<CdEntry>> {
    let mut p = structure.central_offset as usize;
    let cend = p + structure.central_size as usize;
    if cend > data.len() {
        return Err(ParaflateError::ArchiveConsistency(
            "central directory out of range".into(),
        ));
    }
    let mut out = Vec::new();
    for _ in 0..structure.total_entries {
        if p + 46 > cend {
            return Err(ParaflateError::ZipStructure(
                "central record truncated".into(),
            ));
        }
        let sig = read_u32(data, p)?;
        if sig != CENTRAL_SIG {
            return Err(ParaflateError::ZipStructure("central signature".into()));
        }
        let crc32 = read_u32(data, p + 16)?;
        let compressed_size = read_u32(data, p + 20)?;
        let uncompressed_size = read_u32(data, p + 24)?;
        let name_len = read_u16(data, p + 28)? as usize;
        let extra_len = read_u16(data, p + 30)? as usize;
        let comment_len = read_u16(data, p + 32)? as usize;
        let local_rel = read_u32(data, p + 42)? as u64;
        let name_start = p + 46;
        let name_end = name_start + name_len;
        if name_end + extra_len + comment_len > cend {
            return Err(ParaflateError::ZipStructure("central name".into()));
        }
        let name = String::from_utf8_lossy(&data[name_start..name_end]).into_owned();
        let method = read_u16(data, p + 10)?;
        out.push(CdEntry {
            name,
            local_offset: local_rel,
            crc32,
            compressed_size,
            uncompressed_size,
            method,
        });
        p = name_end + extra_len + comment_len;
    }
    if p != cend {
        return Err(ParaflateError::ArchiveConsistency(
            "central directory size mismatch".into(),
        ));
    }
    Ok(out)
}

fn verify_one_entry(data: &[u8], e: &CdEntry, strict: bool) -> ParaflateResult<VerifiedEntry> {
    let lo = e.local_offset as usize;
    if lo + 30 > data.len() {
        return Err(ParaflateError::ArchiveConsistency("local offset".into()));
    }
    let lsig = read_u32(data, lo)?;
    if lsig != LOCAL_SIG {
        return Err(ParaflateError::ZipStructure("local signature".into()));
    }
    let lcrc = read_u32(data, lo + 14)?;
    let lcomp = read_u32(data, lo + 18)?;
    let luncomp = read_u32(data, lo + 22)?;
    let lname_len = read_u16(data, lo + 26)? as usize;
    let lextra_len = read_u16(data, lo + 28)? as usize;
    if strict {
        if lcrc != e.crc32 || lcomp != e.compressed_size || luncomp != e.uncompressed_size {
            return Err(ParaflateError::ArchiveConsistency(format!(
                "local cd mismatch {}",
                e.name
            )));
        }
    }
    let payload_off = lo + 30 + lname_len + lextra_len;
    let payload_end = payload_off + e.compressed_size as usize;
    if payload_end > data.len() {
        return Err(ParaflateError::ArchiveConsistency(format!(
            "payload bounds {}",
            e.name
        )));
    }
    let payload = &data[payload_off..payload_end];
    let plain = match e.method {
        0 => {
            if payload.len() as u32 != e.uncompressed_size {
                return Err(ParaflateError::VerificationFailed {
                    message: "stored size".into(),
                    entry: Some(e.name.clone()),
                });
            }
            payload.to_vec()
        }
        8 => inflate_raw_stream(payload, Some(e.uncompressed_size as usize))?,
        _ => {
            return Err(ParaflateError::VerificationFailed {
                message: format!("unsupported method {}", e.method),
                entry: Some(e.name.clone()),
            });
        }
    };
    let mut h = Hasher::new();
    h.update(&plain);
    let got = h.finalize();
    if got != e.crc32 {
        return Err(ParaflateError::VerificationFailed {
            message: format!("crc {:08x} expected {:08x}", got, e.crc32),
            entry: Some(e.name.clone()),
        });
    }
    Ok(VerifiedEntry {
        name: e.name.clone(),
        crc32: e.crc32,
        compressed_size: e.compressed_size,
        uncompressed_size: e.uncompressed_size,
        method: e.method,
    })
}

pub fn verify_zip_bytes(data: &[u8], strict: bool) -> ParaflateResult<VerificationReport> {
    let structure = scan_end_of_central_directory(data)?;
    let cd = parse_central_directory(data, &structure)?;
    if cd.len() != structure.total_entries as usize {
        return Err(ParaflateError::ArchiveConsistency("entry count".into()));
    }
    let mut inflated_total = 0u64;
    let mut entries = Vec::with_capacity(cd.len());
    for e in &cd {
        let v = verify_one_entry(data, e, strict)?;
        inflated_total = inflated_total.saturating_add(v.uncompressed_size as u64);
        entries.push(v);
    }
    Ok(VerificationReport {
        entries_verified: entries.len() as u64,
        inflated_bytes: inflated_total,
        structure,
        entries,
    })
}

pub fn read_entry_bytes(data: &[u8], entry_name: &str) -> ParaflateResult<Vec<u8>> {
    let structure = scan_end_of_central_directory(data)?;
    let cd = parse_central_directory(data, &structure)?;
    for e in &cd {
        if e.name == entry_name {
            let lo = e.local_offset as usize;
            let lname_len = read_u16(data, lo + 26)? as usize;
            let lextra_len = read_u16(data, lo + 28)? as usize;
            let payload_off = lo + 30 + lname_len + lextra_len;
            let payload_end = payload_off + e.compressed_size as usize;
            let payload = &data[payload_off..payload_end];
            return match e.method {
                0 => {
                    if payload.len() as u32 != e.uncompressed_size {
                        Err(ParaflateError::VerificationFailed {
                            message: "stored size".into(),
                            entry: Some(e.name.clone()),
                        })
                    } else {
                        Ok(payload.to_vec())
                    }
                }
                8 => inflate_raw_stream(payload, Some(e.uncompressed_size as usize)),
                _ => Err(ParaflateError::VerificationFailed {
                    message: "method".into(),
                    entry: Some(e.name.clone()),
                }),
            };
        }
    }
    Err(ParaflateError::EntryNotFound(entry_name.into()))
}
