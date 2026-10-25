//! Terminal layer for Iniquity: CP437, DOS colours, ANSI cursor tracking,
//! and the `|XX` / `%XX` display codes used in prompts and screens.

pub mod codes;
pub mod color;
pub mod cp437;
pub mod screen;

pub use codes::{Control, Source, Token, defuse, lit, tokenize, tokenize_str, tokenize_user};
pub use color::{Color, Palette, Slot};
pub use screen::{Charset, Screen, Shown};
