use std::io::Write;

use paraflate_core::{CompressionMethod, ParaflateError, ParaflateResult};

use crate::record::{CentralDirectoryRecord, LocalHeaderSpec};

const LOCAL_SIG: u32 = 0x04034b50;
const CENTRAL_SIG: u32 = 0x02014b50;
const EOCD_SIG: u32 = 0x06054b50;

#[derive(Clone, Debug, Default)]
pub struct ZipFinalizeSummary {
    pub central_directory_size: u64,
    pub central_directory_offset: u64,
    pub total_entries: u64,
    pub archive_size: u64,
}

pub struct ZipWriter<W: Write> {
    sink: W,
    position: u64,
    central: Vec<CentralDirectoryRecord>,
}

impl<W: Write> ZipWriter<W> {
    pub fn new(sink: W) -> Self {
        Self {
            sink,
            position: 0,
            central: Vec::new(),
        }
    }

    pub fn position(&self) -> u64 {
        self.position
    }

    fn write_all_bytes(&mut self, bytes: &[u8]) -> ParaflateResult<()> {
        self.sink.write_all(bytes)?;
        self.position = self.position.saturating_add(bytes.len() as u64);
        Ok(())
    }

    fn write_u16(&mut self, v: u16) -> ParaflateResult<()> {
        self.write_all_bytes(&v.to_le_bytes())
    }

    fn write_u32(&mut self, v: u32) -> ParaflateResult<()> {
        self.write_all_bytes(&v.to_le_bytes())
    }

    fn method_u16(method: CompressionMethod) -> u16 {
        match method {
            CompressionMethod::Stored => 0,
            CompressionMethod::Deflate => 8,
        }
    }

    pub fn write_local_entry(
        &mut self,
        spec: LocalHeaderSpec,
        payload: &[u8],
    ) -> ParaflateResult<()> {
        if spec.compressed_size as u64 > u32::MAX as u64
            || spec.uncompressed_size as u64 > u32::MAX as u64
        {
            return Err(ParaflateError::ZipStructure(
                "zip32 size overflow".to_string(),
            ));
        }
        let local_offset = self.position;
        let name_bytes = spec.name.as_bytes();
        if spec.name.len() > u16::MAX as usize {
            return Err(ParaflateError::ZipStructure("name too long".to_string()));
        }
        let gpbf: u16 = 0x800;
        self.write_u32(LOCAL_SIG)?;
        self.write_u16(20)?;
        self.write_u16(gpbf)?;
        self.write_u16(Self::method_u16(spec.method))?;
        self.write_u16(spec.dos_time)?;
        self.write_u16(spec.dos_date)?;
        self.write_u32(spec.crc32)?;
        self.write_u32(spec.compressed_size)?;
        self.write_u32(spec.uncompressed_size)?;
        self.write_u16(name_bytes.len() as u16)?;
        self.write_u16(0)?;
        self.write_all_bytes(name_bytes)?;
        self.write_all_bytes(payload)?;
        self.central.push(CentralDirectoryRecord {
            spec: spec.clone(),
            local_header_offset: local_offset,
        });
        Ok(())
    }

    pub fn finalize(mut self) -> ParaflateResult<(W, ZipFinalizeSummary)> {
        let central_offset = self.position;
        let records = self.central.clone();
        for rec in records {
            let name_bytes = rec.spec.name.as_bytes();
            self.write_u32(CENTRAL_SIG)?;
            self.write_u16(0x0314)?;
            self.write_u16(20)?;
            self.write_u16(0x800)?;
            self.write_u16(Self::method_u16(rec.spec.method))?;
            self.write_u16(rec.spec.dos_time)?;
            self.write_u16(rec.spec.dos_date)?;
            self.write_u32(rec.spec.crc32)?;
            self.write_u32(rec.spec.compressed_size)?;
            self.write_u32(rec.spec.uncompressed_size)?;
            self.write_u16(name_bytes.len() as u16)?;
            self.write_u16(0)?;
            self.write_u16(0)?;
            self.write_u16(0)?;
            self.write_u16(0)?;
            self.write_u32(0)?;
            self.write_u32(rec.local_header_offset as u32)?;
            self.write_all_bytes(name_bytes)?;
        }
        let central_size = self.position.saturating_sub(central_offset);
        let total = self.central.len() as u64;
        self.write_u32(EOCD_SIG)?;
        self.write_u16(0)?;
        self.write_u16(0)?;
        self.write_u16(total as u16)?;
        self.write_u16(total as u16)?;
        self.write_u32(central_size as u32)?;
        self.write_u32(central_offset as u32)?;
        self.write_u16(0)?;
        let archive_size = self.position;
        let summary = ZipFinalizeSummary {
            central_directory_size: central_size,
            central_directory_offset: central_offset,
            total_entries: total,
            archive_size,
        };
        Ok((self.sink, summary))
    }
}
