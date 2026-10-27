//! Application shell UI components.

pub mod empty;
pub mod player_bar;
pub mod section;
pub mod top_bar;
pub mod track_row;

pub(crate) use empty::empty_state;
pub(crate) use player_bar::render_player_bar;
pub(crate) use section::section_header;
pub(crate) use top_bar::render_top_bar;
pub(crate) use track_row::track_row;
