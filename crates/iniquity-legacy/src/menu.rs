//! *.MNU: a `tMenuRec` header (743 bytes) followed by command records.
//!
//! a26's `tCommandRec` was 218 bytes; r5 appends lightbar fields
//! (X, Y, then ON and OFF strings), making it 422.

use crate::pascal::{Reader, check};

pub const HEADER_SIZE: usize = 743;
pub const COMMAND_SIZE: usize = 422;

#[derive(Debug, Clone)]
pub struct LegacyMenu {
    /// 1 normal, 2 generic, 3 lightbar
    pub menu_type: u8,
    pub title: String,
    pub prompt_name: String,
    pub help_file: String,
    pub prompt: String,
    pub acs: String,
    pub password: String,
    pub fallback: String,
    /// 1 user setting, 2 forced, 3 disabled
    pub expert: u8,
    pub generic_columns: u8,
    /// 1 user setting, 2 forced, 3 disabled
    pub hot_key: u8,
    pub clear_before: bool,
    pub center_title: bool,
    pub show_prompt: bool,
    pub pause_before: bool,
    pub global_use: bool,
    pub input_upper: bool,
    pub commands: Vec<LegacyCommand>,
}

#[derive(Debug, Clone)]
pub struct LegacyCommand {
    pub desc: String,
    pub help: String,
    pub keys: String,
    pub acs: String,
    pub command: String,
    pub param: String,
    pub hidden: bool,
    pub x: u8,
    pub y: u8,
    pub on: String,
    pub off: String,
}

impl LegacyMenu {
    pub fn parse(buf: &[u8]) -> Result<Self, crate::Error> {
        check("tMenuRec", buf, HEADER_SIZE)?;
        let mut r = Reader::new(buf);
        let mut menu = LegacyMenu {
            menu_type: r.u8(),
            title: r.string(255),
            prompt_name: r.string(60),
            help_file: r.string(8),
            prompt: r.string(255),
            acs: r.string(20),
            password: r.string(20),
            fallback: r.string(8),
            expert: r.u8(),
            generic_columns: r.u8(),
            hot_key: r.u8(),
            clear_before: r.bool(),
            center_title: r.bool(),
            show_prompt: r.bool(),
            pause_before: r.bool(),
            global_use: r.bool(),
            input_upper: r.bool(),
            commands: Vec::new(),
        };
        for rec in buf[HEADER_SIZE..].as_chunks::<COMMAND_SIZE>().0 {
            let mut r = Reader::new(rec);
            menu.commands.push(LegacyCommand {
                desc: r.string(35),
                help: r.string(70),
                keys: r.string(14),
                acs: r.string(20),
                command: r.string(2),
                param: r.string(70),
                hidden: r.bool(),
                x: r.u8(),
                y: r.u8(),
                on: r.string(100),
                off: r.string(100),
            });
        }
        Ok(menu)
    }
}
