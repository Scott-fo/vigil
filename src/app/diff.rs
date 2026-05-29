use super::*;

mod cache;
mod highlight;
mod load;
mod viewport;

#[cfg(test)]
pub(super) use self::cache::DIFF_CACHE_CAPACITY;
pub use self::cache::DiffCacheKey;
pub(super) use self::cache::{DIFF_PREFETCH_DISTANCE, DiffViewCache};
pub(super) use self::highlight::DiffHighlightJob;
pub(super) use self::viewport::DiffViewport;
pub use self::viewport::PreparedDiffViewport;

#[cfg(test)]
mod tests;
