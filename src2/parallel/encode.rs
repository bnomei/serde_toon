//! Parallel encode helpers.

#[cfg(feature = "parallel")]
use rayon::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeThresholds {
    pub min_items: usize,
}

pub fn should_parallelize(total_items: usize, thresholds: EncodeThresholds) -> bool {
    total_items >= thresholds.min_items
}

#[cfg(feature = "parallel")]
pub fn map_items_parallel<T, R, F>(items: &[T], func: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync + Send,
{
    items.par_iter().map(func).collect()
}

#[cfg(not(feature = "parallel"))]
pub fn map_items_parallel<T, R, F>(items: &[T], func: F) -> Vec<R>
where
    F: Fn(&T) -> R,
{
    items.iter().map(func).collect()
}
