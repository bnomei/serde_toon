//! Parallel decode helpers.

#[cfg(feature = "parallel")]
use rayon::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeThresholds {
    pub min_lines: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockSpan {
    pub start: usize,
    pub end: usize,
    pub indent: usize,
}

pub fn should_parallelize(total_lines: usize, thresholds: DecodeThresholds) -> bool {
    total_lines >= thresholds.min_lines
}

pub fn block_spans_from<F>(
    start: usize,
    len: usize,
    base_indent: usize,
    indent_at: F,
) -> Vec<BlockSpan>
where
    F: Fn(usize) -> usize,
{
    let mut spans = Vec::new();
    let mut i = start;
    while i < len {
        if indent_at(i) != base_indent {
            break;
        }
        let block_start = i;
        i += 1;
        while i < len && indent_at(i) > base_indent {
            i += 1;
        }
        spans.push(BlockSpan {
            start: block_start,
            end: i,
            indent: base_indent,
        });
    }
    spans
}

#[cfg(feature = "parallel")]
pub fn map_spans_parallel<R, F>(spans: &[BlockSpan], func: F) -> Vec<R>
where
    R: Send,
    F: Fn(&BlockSpan) -> R + Sync + Send,
{
    spans.par_iter().map(func).collect()
}

#[cfg(not(feature = "parallel"))]
pub fn map_spans_parallel<R, F>(spans: &[BlockSpan], func: F) -> Vec<R>
where
    F: Fn(&BlockSpan) -> R,
{
    spans.iter().map(func).collect()
}
