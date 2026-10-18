//! INIQUITY.DAT: one `tCfgRec` (10000 bytes). Only fields the port uses
//! are kept; the rest are walked over so offsets stay right.

use iniquity_term::Color;

use crate::pascal::{Reader, check};

pub const SIZE: usize = 10000;
const MAX_ADDRESS: usize = 20;
const MAX_ORIGIN: usize = 20;

#[derive(Debug, Clone)]
pub struct LegacyConfig {
    pub bbs_name: String,
    pub bbs_phone: String,
    pub sysop_alias: String,
    pub sysop_name: String,
    pub system_pw: String,
    pub acs_sysop: String,
    pub acs_cosysop: String,
    pub real_name_system: bool,
    pub default_colors: [Color; 11],
    pub matrix_login: bool,
    pub ask_apply: bool,
    pub time_limit_per_call: bool,
    pub acs_system_pw_login: String,
    pub calls_birth: u8,
    pub calls_phone: u8,
    pub login_tries: u8,
    pub origins: Vec<String>,
    pub pw_echo_char: char,
    pub remove_pause: bool,
    pub num_last_calls: u8,
    pub new_user_pw: String,
    pub alias_format: u8,
    pub default_page_len: u8,
    pub new_expert: bool,
    pub new_yes_no_bars: bool,
    pub new_hot_keys: bool,
    pub new_ask_expert: bool,
    pub new_ask_yes_no_bars: bool,
    pub new_ask_hot_keys: bool,
    pub new_ask_page_len: bool,
    pub start_menu: String,
    pub new_pause: bool,
    pub new_quote: bool,
    pub new_ask_pause: bool,
    pub new_ask_quote: bool,
    pub bbs_location: String,
    pub light_char: char,
    pub light_char_ok: char,
    pub light_char_fail: char,
    pub new_config: bool,
    pub new_verify: bool,
    pub pmt_yes: String,
    pub pmt_no: String,
    pub pmt_yes_word: String,
    pub pmt_no_word: String,
    pub pmt_yes_bar: String,
    pub pmt_no_bar: String,
    pub chat_start: String,
    pub chat_end: String,
    /// Paging the SysOp (Chat mode setup).
    pub no_chat_pw: String,
    pub chat_page_noise: bool,
    pub max_page_times: u8,
    pub max_page_beeps: u8,
    pub restore_chat_time: bool,
    pub chat_over_acs: String,
    pub page_ask_email: bool,
    pub acs_attach_public: String,
    pub acs_attach_email: String,
    pub ask_upload_reply: bool,
    pub ask_upload_email: bool,
    pub inact_time: bool,
    pub inact_seconds: u16,
    pub inact_warning: u16,
    pub nuv_validation: bool,
    pub nuv_votes_yes: u8,
    pub nuv_votes_no: u8,
    /// Who gets voted on.
    pub nuv_access: String,
    /// Who may vote.
    pub nuv_vote_access: String,
    pub nuv_initials: bool,
    /// Level given when voted in.
    pub nuv_user_level: char,
    pub multi_node: bool,
    pub acs_post_email: String,
    pub acs_anonymous: String,
    pub acs_upload_message: String,
    pub acs_autosig: String,
    pub abort_mandatory_ok: bool,
    pub ask_private_msg: bool,
    pub ask_private_reply: bool,
    pub ask_post_in_area: bool,
    pub ask_kill_msg: bool,
    pub ask_kill_all_msg: bool,
    pub ask_auto_quote: bool,
    pub default_quote_num: bool,
    pub max_quote_lines: u8,
    pub conf_ignore_msg: bool,
    pub comp_msg_areas: bool,
    /// Shown in place of quoted lines that contain ANSI codes.
    pub ansi_string: String,
    pub kb_per_file_point: u16,
    pub use_file_points: bool,
    pub import_descs: bool,
    pub use_dl_limit: bool,
    pub use_dl_kb_limit: bool,
    pub no_desc_line: String,
    /// QWK packet name (BBSID), and the screens put in each packet.
    pub qwk_filename: String,
    pub qwk_welcome: String,
    pub qwk_news: String,
    pub qwk_goodbye: String,
    pub qwk_local_path: String,
    pub qwk_ignore_time: bool,
    pub qwk_strip_sigs: bool,
    /// 1 none, 2 this area, 3 all areas, 4 all conferences.
    pub ul_search: u8,
    pub auto_validate: bool,
    /// Percent of a file's points awarded to the uploader.
    pub file_points_per: u16,
    pub use_uldl_ratio: bool,
    pub use_kb_ratio: bool,
    pub file_desc1: String,
    pub file_desc2: String,
    pub desc_wrap: bool,
    pub adv_file_bar: bool,
    pub allow_blind: bool,
    pub acs_anon_automsg: String,
}

impl LegacyConfig {
    pub fn parse(buf: &[u8]) -> Result<Self, crate::Error> {
        check("tCfgRec", buf, SIZE)?;
        let mut r = Reader::new(buf);
        let bbs_name = r.string(40);
        let bbs_phone = r.string(13);
        let sysop_alias = r.string(36);
        let sysop_name = r.string(36);
        let system_pw = r.string(20);
        let acs_sysop = r.string(20);
        let acs_cosysop = r.string(20);
        r.skip(3); // DirectWrites, SnowChecking, OverlayToEMS
        let real_name_system = r.bool();
        r.skip(1); // ShowPwLocal
        r.skip(7); // status bar settings
        let default_colors = std::array::from_fn(|_| r.color());
        r.skip(MAX_ADDRESS * 8);
        r.skip(7); // ESCtoExit .. DefWFCstat (incl. BlankSeconds word)
        r.skip(10 * 41); // paths
        r.skip(4 + 3); // logging, swapping
        r.skip(21 * 2); // BbsAccessPw, NoBaudPW
        r.skip(1); // SysOpAutoLogin
        let matrix_login = r.bool();
        let ask_apply = r.bool();
        let time_limit_per_call = r.bool();
        let acs_system_pw_login = r.string(20);
        let calls_birth = r.u8();
        let calls_phone = r.u8();
        let login_tries = r.u8();
        let origins = (0..MAX_ORIGIN).map(|_| r.string(75)).filter(|s| !s.is_empty()).collect();
        let no_chat_pw = r.string(20);
        let chat_page_noise = r.bool();
        let max_page_times = r.u8();
        let max_page_beeps = r.u8();
        let pw_echo_char = r.char();
        let remove_pause = r.bool();
        r.skip(1); // AddLocalCalls
        let num_last_calls = r.u8();
        let acs_post_email = r.string(20);
        let acs_anonymous = r.string(20);
        let acs_anon_automsg = r.string(20);
        let acs_upload_message = r.string(20);
        let acs_autosig = r.string(20);
        let abort_mandatory_ok = r.bool();
        let ask_private_msg = r.bool();
        let ask_private_reply = r.bool();
        let ask_post_in_area = r.bool();
        let ask_upload_reply = r.bool();
        let ask_upload_email = r.bool();
        let ask_kill_msg = r.bool();
        let ask_kill_all_msg = r.bool();
        let new_user_pw = r.string(20);
        let alias_format = r.u8();
        let default_page_len = r.u8();
        let new_expert = r.bool();
        let new_yes_no_bars = r.bool();
        let new_hot_keys = r.bool();
        let new_ask_expert = r.bool();
        let new_ask_yes_no_bars = r.bool();
        let new_ask_hot_keys = r.bool();
        let new_ask_page_len = r.bool();
        let start_menu = r.string(8);
        r.skip(10 * 256); // Macro
        r.skip(41 + 1); // pathArch, ArchiverSwap
        let new_pause = r.bool();
        let new_quote = r.bool();
        let new_ask_pause = r.bool();
        let new_ask_quote = r.bool();
        let ask_auto_quote = r.bool();
        let default_quote_num = r.bool();
        let max_quote_lines = r.u8();
        r.skip(1 + 41); // iniqAsDoor, pathAtch
        let acs_attach_public = r.string(20);
        let acs_attach_email = r.string(20);
        let conf_ignore_msg = r.bool();
        let comp_msg_areas = r.bool();
        r.skip(1); // compFileAreas
        let restore_chat_time = r.bool();
        let kb_per_file_point = r.u16();
        let use_file_points = r.bool();
        let import_descs = r.bool();
        let use_dl_limit = r.bool();
        let use_dl_kb_limit = r.bool();
        let bbs_location = r.string(40);
        let qwk_filename = r.string(8);
        let qwk_welcome = r.string(12);
        let qwk_news = r.string(12);
        let qwk_goodbye = r.string(12);
        let qwk_local_path = r.string(40);
        let qwk_ignore_time = r.bool();
        let qwk_strip_sigs = r.bool();
        let no_desc_line = r.string(50);
        r.skip(2 + 2); // waitConnect, modemReInit
        let light_char = r.char();
        let light_char_ok = r.char();
        let light_char_fail = r.char();
        r.skip(51 + 3 + 13 * 3 + 1); // virusScan .. extMaint
        let ul_search = r.u8();
        let auto_validate = r.bool();
        let file_points_per = r.u16();
        let use_uldl_ratio = r.bool();
        let use_kb_ratio = r.bool();
        let file_desc1 = r.string(13);
        let file_desc2 = r.string(13);
        r.skip(1 + 41 + 1); // useTextLibs, pathLibs, echomailLev
        let new_config = r.bool();
        let new_verify = r.bool();
        let pmt_yes = r.string(30);
        let pmt_no = r.string(30);
        let pmt_yes_word = r.string(20);
        let pmt_no_word = r.string(20);
        let pmt_yes_bar = r.string(30);
        let pmt_no_bar = r.string(30);
        let desc_wrap = r.bool();
        let chat_start = r.string(5);
        let chat_end = r.string(5);
        let chat_over_acs = r.string(20);
        let adv_file_bar = r.bool();
        let inact_time = r.bool();
        r.skip(1); // inactInChat
        let inact_seconds = r.u16();
        let inact_warning = r.u16();
        let ansi_string = r.string(75);
        let page_ask_email = r.bool();
        r.skip(2); // soundRestrict, inactLocal
        let allow_blind = r.bool();
        let nuv_votes_yes = r.u8();
        let nuv_votes_no = r.u8();
        let nuv_access = r.string(20);
        let nuv_vote_access = r.string(20);
        let nuv_initials = r.bool();
        let nuv_user_level = r.u8() as char;
        let nuv_validation = r.bool();
        let multi_node = r.bool();
        debug_assert_eq!(r.pos(), 5992);
        Ok(Self {
            bbs_name, bbs_phone, sysop_alias, sysop_name, system_pw, acs_sysop, acs_cosysop,
            real_name_system, default_colors, matrix_login, ask_apply, time_limit_per_call,
            acs_system_pw_login, calls_birth, calls_phone, login_tries, origins, pw_echo_char,
            remove_pause, num_last_calls, new_user_pw, alias_format, default_page_len,
            new_expert, new_yes_no_bars, new_hot_keys, new_ask_expert, new_ask_yes_no_bars,
            new_ask_hot_keys, new_ask_page_len, start_menu, new_pause, new_quote,
            new_ask_pause, new_ask_quote, bbs_location, light_char, light_char_ok,
            light_char_fail, new_config, new_verify, pmt_yes, pmt_no, pmt_yes_word,
            pmt_no_word, pmt_yes_bar, pmt_no_bar, chat_start, chat_end, inact_time,
            inact_seconds, inact_warning, nuv_validation, nuv_votes_yes, nuv_votes_no, nuv_access,
            nuv_vote_access, nuv_initials, nuv_user_level, multi_node, acs_post_email,
            acs_anonymous, acs_upload_message, acs_autosig, abort_mandatory_ok, ask_private_msg,
            ask_private_reply, ask_post_in_area, ask_kill_msg, ask_kill_all_msg, ask_auto_quote,
            default_quote_num, max_quote_lines, conf_ignore_msg, comp_msg_areas, ansi_string,
            kb_per_file_point, use_file_points, import_descs, use_dl_limit, use_dl_kb_limit,
            no_desc_line, ul_search, auto_validate, file_points_per, use_uldl_ratio, use_kb_ratio,
            file_desc1, file_desc2, desc_wrap, adv_file_bar, allow_blind, acs_anon_automsg,
            qwk_filename, qwk_welcome, qwk_news, qwk_goodbye, qwk_local_path, qwk_ignore_time,
            qwk_strip_sigs, no_chat_pw, chat_page_noise, max_page_times, max_page_beeps,
            restore_chat_time, chat_over_acs, page_ask_email, acs_attach_public, acs_attach_email,
            ask_upload_reply, ask_upload_email,
        })
    }
}
