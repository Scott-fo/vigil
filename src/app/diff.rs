use super::*;

mod cache;
mod highlight;
mod load;
mod snapshot;
mod stats;
mod viewport;

#[cfg(test)]
pub(super) use self::cache::DIFF_CACHE_CAPACITY;
pub use self::cache::DiffCacheKey;
pub(super) use self::cache::{
    DIFF_DIRECTIONAL_PREFETCH_DISTANCE, DIFF_PREFETCH_DISTANCE, DiffPrefetchDirection,
    DiffViewCache,
};
pub(super) use self::highlight::DiffHighlightJob;
pub use self::stats::DiffStatsState;
pub(super) use self::viewport::DiffViewport;
pub use self::viewport::PreparedDiffViewport;

#[cfg(test)]
mod tests;
