use paraflate_core::CompressionMethod;

#[derive(Clone, Debug)]
pub struct LocalHeaderSpec {
    pub name: String,
    pub method: CompressionMethod,
    pub crc32: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub dos_time: u16,
    pub dos_date: u16,
}

#[derive(Clone, Debug)]
pub struct CentralDirectoryRecord {
    pub spec: LocalHeaderSpec,
    pub local_header_offset: u64,
}
