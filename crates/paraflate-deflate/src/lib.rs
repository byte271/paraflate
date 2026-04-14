mod bit_writer;
mod dynamic;
mod dynamic_block;
mod engine;
mod fixed;
mod huffman;
mod plan;
mod pm;
mod stream;
mod tables;

pub use dynamic::length_limited_lengths;
pub use dynamic_block::{aggregate_freq, build_dynamic_trees, DynamicTrees};
pub use engine::{
    DeflateEngine, DeflateEngineConfig, DeflateOutput, EntryCompressHints, Lz77JobOutput,
};
pub use plan::BlockPlanner;
pub use pm::package_merge;
pub use stream::{encode_deflate_blocks, encode_one_deflate_block, DeflateEncodeOptions};
