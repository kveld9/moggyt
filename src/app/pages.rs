//! Application shell page views.

pub(crate) mod now_playing;
pub(crate) mod queue;
pub(crate) mod search;

pub(crate) use now_playing::render_now_playing;
pub(crate) use queue::render_queue;
pub(crate) use search::render_search;
