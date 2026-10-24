//! Screens. Each page module exposes `show`-style functions called by the app shell.

pub mod widgets;
#[cfg(not(target_arch = "wasm32"))]
pub mod software_update;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod release_notes;

pub mod accounts;
pub mod agents;
pub mod assistant;
pub mod audiobooks;
pub mod chat_kit;
pub mod curator;
pub mod detail;
pub mod info;
pub mod library;
pub mod listen_now;
pub mod lyrics;
pub mod now_playing;
pub mod player_bar;
pub mod queue;
pub mod radio;
pub mod search;
pub mod settings;
pub mod sidebar;
pub mod sound;
pub mod statistics;
pub mod welcome;
