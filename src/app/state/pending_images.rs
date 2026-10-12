//! Memory-only clipboard images awaiting an accepted pane submission.

/// An image spyc captured off the system clipboard as the user pasted it into
/// an agent pane — before the agent has sent it, and therefore before it exists
/// anywhere else spyc can read.
///
/// Memory-only, deliberately: these are the user's screenshots, and nothing
/// writes them to disk unless the user asks (`s` in the overlay).
pub struct PendingImage {
    /// 1-based within the tab, counting from the last submitted prompt.
    pub seq: usize,
    /// PNG. `arboard` returns RGBA, so this was encoded once on the worker
    /// rather than kept raw — a 4K screenshot is 33 MB unencoded.
    pub encoded: Vec<u8>,
    pub dims: (u32, u32),
    pub captured_at: u64,
}

/// Most uncommitted images kept per tab. Past this the oldest is dropped: a
/// prompt carrying more than a handful of images is not a case worth spending
/// hundreds of MB on.
pub const MAX_PENDING_IMAGES_PER_TAB: usize = 8;
