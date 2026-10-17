//! `iniquity import <old install> [dest]`: convert an Iniquity 1.00a26
//! (DOS) installation into this port's files.
//!
//! | from                  | to                          |
//! |-----------------------|-----------------------------|
//! | INIQUITY.DAT          | iniquity.toml               |
//! | DATA\LEVELS.DAT       | iniquity.toml `[levels.X]`  |
//! | DATA\STRINGS.DAT      | strings.toml                |
//! | DATA\USERS.DAT        | data/iniquity.db            |
//! | DATA\MSGAREA.DAT etc. | msgareas.toml               |
//! | MSGS\*.HDR, *.DAT     | data/iniquity.db            |
//! | MENU\*.MNU            | menus/*.toml                |
//! | TEXT\*                | text/                       |

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use iniquity_legacy::msgarea::{flag, status};
use iniquity_legacy::{LegacyConf, LegacyConfig, LegacyLevel, LegacyMenu, LegacyMsgArea, LegacyMsgHeader, LegacyUser, strings, user::ac};
use iniquity_term::Palette;

use crate::config::{self, Config, Level};
use crate::menu::{Command, Menu, MenuType, Mode};
use crate::msgareas::{Conf, MsgArea, MsgAreas};
use crate::msgbase::{Message, MsgBase, Party};
use crate::session::find_ci;
use crate::user::{User, parse_date};
use crate::users::UserStore;

pub fn run(src: &Path, dest: &Path, force: bool) -> anyhow::Result<()> {
    let cfg_path = dest.join("iniquity.toml");
    let db_path = dest.join("data/iniquity.db");
    if !force && (cfg_path.exists() || db_path.exists()) {
        bail!("{} already has iniquity.toml or data/iniquity.db; use --force to replace them", dest.display());
    }
    let dir = |name: &str| find_ci(src, name).filter(|p| p.is_dir());
    let data = dir("DATA").context("no DATA directory")?;
    let file = |d: &Path, name: &str| find_ci(d, name).with_context(|| format!("{name} not found in {}", d.display()));

    let legacy = LegacyConfig::parse(&std::fs::read(file(src, "INIQUITY.DAT")?)?)?;
    let levels = match find_ci(&data, "LEVELS.DAT") {
        Some(p) => LegacyLevel::parse_file(&std::fs::read(p)?)?,
        None => Vec::new(),
    };
    std::fs::create_dir_all(dest)?;

    let mut cfg = convert_config(&legacy, &levels);
    if let Ok(existing) = Config::load(&cfg_path) {
        // Keep transport settings from a config we're replacing.
        cfg.ssh = existing.ssh;
        cfg.terminal = existing.terminal;
    }
    let header = format!(
        "# Iniquity BBS configuration, imported from {}.\n# Paths are relative to this file.\n\n",
        src.display()
    );
    std::fs::write(&cfg_path, header + &toml::to_string_pretty(&cfg)?)?;
    println!("wrote {}", cfg_path.display());

    if let Some(p) = find_ci(&data, "STRINGS.DAT") {
        let table = strings::parse_file(&std::fs::read(p)?);
        let entries: Vec<(String, String)> = table
            .into_iter()
            .enumerate()
            .map(|(i, s)| (strings::NAMES.get(i).map_or_else(|| format!("s{}", i + 1), |n| n.to_string()), s))
            .collect();
        let out = dest.join("strings.toml");
        std::fs::write(&out, crate::strings::to_toml(&entries))?;
        println!("wrote {} ({} strings)", out.display(), entries.len());
    }

    if let Some(menus) = dir("MENU") {
        let out = dest.join("menus");
        std::fs::create_dir_all(&out)?;
        let mut n = 0;
        for entry in std::fs::read_dir(&menus)? {
            let path = entry?.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mnu")) {
                continue;
            }
            let menu = convert_menu(&LegacyMenu::parse(&std::fs::read(&path)?)?);
            std::fs::write(out.join(format!("{}.toml", stem.to_ascii_uppercase())), menu.to_toml())?;
            n += 1;
        }
        println!("wrote {n} menus to {}", out.display());
    }

    if let Some(text) = dir("TEXT") {
        let out = dest.join("text");
        std::fs::create_dir_all(&out)?;
        let mut n = 0;
        for entry in std::fs::read_dir(&text)? {
            let path = entry?.path();
            if path.is_file() {
                std::fs::copy(&path, out.join(path.file_name().unwrap()))?;
                n += 1;
            }
        }
        println!("copied {n} text files to {}", out.display());
    }

    if force && db_path.exists() {
        std::fs::remove_file(&db_path)?;
    }
    let store = UserStore::open(&db_path)?;
    let mut n = 0;
    if let Some(p) = find_ci(&data, "USERS.DAT") {
        for (i, lu) in LegacyUser::parse_file(&std::fs::read(p)?)?.iter().enumerate() {
            if lu.user_name.trim().is_empty() {
                continue;
            }
            let mut u = convert_user(lu);
            u.id = i as u32 + 1;
            store.insert(&mut u).with_context(|| format!("importing user {}", lu.user_name))?;
            n += 1;
        }
    }
    println!("imported {n} users into {}", db_path.display());

    // Message conferences and areas.
    let read = |name: &str| find_ci(&data, name).map(std::fs::read).transpose();
    let mut areas = MsgAreas::default();
    if let Some(buf) = read("MSGCONF.DAT")? {
        let confs: Vec<Conf> =
            LegacyConf::parse_file(&buf).into_iter().map(|c| Conf { key: c.key, desc: c.desc, acs: c.acs }).collect();
        if !confs.is_empty() {
            areas.confs = confs;
        }
    }
    if let Some(buf) = read("MSGAREA.DAT")? {
        areas.areas = LegacyMsgArea::parse_file(&buf)?.iter().map(convert_area).collect();
    }
    if let Some(email) = read("EMAIL.DAT")?.map(|b| LegacyMsgArea::parse_file(&b)).transpose()?.and_then(|v| v.into_iter().next()) {
        areas.email = convert_area(&email);
    }
    let out = dest.join("msgareas.toml");
    std::fs::write(&out, areas.to_toml())?;
    println!("wrote {} ({} areas, {} conferences)", out.display(), areas.areas.len(), areas.confs.len());

    // Message bases, from the a26 message directory if present.
    let base = MsgBase::open(&db_path)?;
    let mut n = 0;
    if let Some(msgs) = dir("MSGS") {
        for area in std::iter::once(&areas.email).chain(&areas.areas) {
            let (Some(hdr), Some(dat)) =
                (find_ci(&msgs, &format!("{}.HDR", area.file)), find_ci(&msgs, &format!("{}.DAT", area.file)))
            else {
                continue;
            };
            let dat = std::fs::read(dat)?;
            for (i, h) in LegacyMsgHeader::parse_file(&std::fs::read(hdr)?)?.iter().enumerate() {
                let mut m = convert_message(&area.file, i as u32 + 1, h, &dat);
                base.add(&mut m).with_context(|| format!("importing {} #{}", area.file, i + 1))?;
                n += 1;
            }
        }
    }
    println!("imported {n} messages");
    Ok(())
}

fn convert_config(c: &LegacyConfig, levels: &[LegacyLevel]) -> Config {
    let mut cfg = Config::default();
    cfg.bbs.name = c.bbs_name.clone();
    cfg.bbs.location = c.bbs_location.clone();
    cfg.bbs.phone = c.bbs_phone.clone();
    cfg.bbs.sysop_alias = c.sysop_alias.clone();
    cfg.bbs.sysop_name = c.sysop_name.clone();
    cfg.bbs.inactivity_seconds = if c.inact_time { c.inact_seconds as u64 } else { 0 };
    cfg.acs.sysop = c.acs_sysop.clone();
    cfg.acs.cosysop = c.acs_cosysop.clone();
    cfg.logon.matrix = c.matrix_login;
    cfg.logon.ask_apply = c.ask_apply;
    cfg.logon.tries = c.login_tries;
    cfg.logon.system_password = c.system_pw.clone();
    cfg.logon.system_password_acs = c.acs_system_pw_login.clone();
    cfg.logon.start_menu = c.start_menu.clone();
    cfg.logon.real_names = c.real_name_system;
    cfg.logon.time_per_call = c.time_limit_per_call;
    let n = &mut cfg.newuser;
    n.password = c.new_user_pw.clone();
    n.verify = c.new_verify;
    n.alias_format = c.alias_format;
    n.page_length = c.default_page_len as u16;
    n.ask_page_length = c.new_ask_page_len;
    n.hot_keys = c.new_hot_keys;
    n.ask_hot_keys = c.new_ask_hot_keys;
    n.expert = c.new_expert;
    n.ask_expert = c.new_ask_expert;
    n.pause = c.new_pause;
    n.ask_pause = c.new_ask_pause;
    n.quote = c.new_quote;
    n.ask_quote = c.new_ask_quote;
    n.yes_no_bars = c.new_yes_no_bars;
    n.ask_yes_no_bars = c.new_ask_yes_no_bars;
    let p = &mut cfg.prompts;
    p.yes = c.pmt_yes.clone();
    p.no = c.pmt_no.clone();
    p.yes_word = c.pmt_yes_word.clone();
    p.no_word = c.pmt_no_word.clone();
    p.yes_bar = c.pmt_yes_bar.clone();
    p.no_bar = c.pmt_no_bar.clone();
    p.password_echo = c.pw_echo_char;
    p.remove_pause = c.remove_pause;
    cfg.colors = Palette(c.default_colors);
    let m = &mut cfg.messages;
    m.acs_post_email = c.acs_post_email.clone();
    m.acs_anonymous = c.acs_anonymous.clone();
    m.acs_autosig = c.acs_autosig.clone();
    m.abort_mandatory_ok = c.abort_mandatory_ok;
    m.ask_private = c.ask_private_msg;
    m.ask_private_reply = c.ask_private_reply;
    m.ask_post_after_read = c.ask_post_in_area;
    m.ask_delete_replied = c.ask_kill_msg;
    m.ask_delete_all_read = c.ask_kill_all_msg;
    m.ask_auto_quote = c.ask_auto_quote;
    m.default_quote_lines = c.default_quote_num;
    m.max_quote_lines = c.max_quote_lines;
    m.mandatory_all_confs = c.conf_ignore_msg;
    m.compress_area_numbers = c.comp_msg_areas;
    m.ansi_quote = c.ansi_string.clone();
    cfg.levels = if levels.is_empty() {
        config::default_levels()
    } else {
        levels
            .iter()
            .map(|l| {
                (l.key.to_string(), Level {
                    desc: l.desc.clone(),
                    sl: l.sl,
                    dsl: l.dsl,
                    time_limit: l.time_limit,
                    file_points: l.file_points,
                    post_call: l.post_call,
                    limit_dl: l.limit_dl,
                    limit_dl_kb: l.limit_dl_kb,
                    note: l.user_note.clone(),
                    uldl_ratio: l.uldl_ratio,
                    kb_ratio: l.kb_ratio,
                })
            })
            .collect()
    };
    cfg
}

fn convert_area(a: &LegacyMsgArea) -> MsgArea {
    MsgArea {
        name: a.name.clone(),
        file: a.filename.to_ascii_uppercase(),
        sponsor: a.sponsor.clone(),
        acs: a.acs.clone(),
        post_acs: a.post_acs.clone(),
        password: a.password.clone(),
        unhidden: a.has(flag::UNHIDDEN),
        real_names: a.has(flag::REAL_NAME),
        private: a.has(flag::PRIVATE),
        mandatory: a.has(flag::MANDATORY),
        anonymous: a.has(flag::ANONYMOUS),
        qwk_name: a.qwk_name.clone(),
    }
}

fn convert_party(i: &iniquity_legacy::msgarea::LegacyMsgInfo) -> Party {
    Party { user_id: i.user_num as u32, alias: i.alias.clone(), real_name: i.real_name.clone(), name: i.name.clone(), note: i.user_note.clone() }
}

fn convert_message(area: &str, num: u32, h: &LegacyMsgHeader, dat: &[u8]) -> Message {
    let lines = h.lines(dat);
    // a26's sigPos is the line before the signature.
    let sig_pos = if h.sig_pos > 0 && (h.sig_pos as usize) < lines.len() { h.sig_pos as u32 + 1 } else { 0 };
    Message {
        area: area.to_string(),
        num,
        from: convert_party(&h.from),
        to: convert_party(&h.to),
        subject: h.subject.clone(),
        posted: h.date.unwrap_or_default(),
        deleted: h.has(status::DELETED),
        private: h.has(status::PRIVATE),
        anonymous: h.has(status::ANONYMOUS),
        replies: h.replies as u32,
        sig_pos,
        lines,
    }
}

fn mode(v: u8) -> Mode {
    match v {
        2 => Mode::Forced,
        3 => Mode::Disabled,
        _ => Mode::User,
    }
}

fn convert_menu(m: &LegacyMenu) -> Menu {
    Menu {
        menu_type: match m.menu_type {
            2 => MenuType::Generic,
            3 => MenuType::Lightbar,
            _ => MenuType::Normal,
        },
        title: m.title.clone(),
        prompt_name: m.prompt_name.clone(),
        help_file: m.help_file.clone(),
        prompt: m.prompt.clone(),
        acs: m.acs.clone(),
        password: m.password.clone(),
        fallback: m.fallback.clone(),
        expert: mode(m.expert),
        hot_keys: mode(m.hot_key),
        generic_columns: m.generic_columns,
        clear_before: m.clear_before,
        center_title: m.center_title,
        show_prompt: m.show_prompt,
        pause_before: m.pause_before,
        global: m.global_use,
        upper_input: m.input_upper,
        commands: m
            .commands
            .iter()
            .map(|c| Command {
                keys: c.keys.clone(),
                command: c.command.clone(),
                param: c.param.clone(),
                acs: c.acs.clone(),
                desc: c.desc.clone(),
                help: c.help.clone(),
                hidden: c.hidden,
                on: c.on.clone(),
                off: c.off.clone(),
            })
            .collect(),
    }
}

fn convert_user(l: &LegacyUser) -> User {
    let mut u = User {
        id: 0,
        handle: l.user_name.clone(),
        real_name: l.real_name.clone(),
        password_hash: String::new(),
        phone: l.phone.clone(),
        birth_date: parse_date(&l.birth_date),
        location: l.location.clone(),
        address: l.address.clone(),
        zip_code: l.zip_code.clone(),
        note: l.user_note.clone(),
        sysop_note: l.sysop_note.clone(),
        sex: l.sex,
        sl: l.sl,
        dsl: l.dsl,
        level: l.level,
        flags: l.flags.iter().copied().collect(),
        hot_keys: l.has(ac::HOT_KEY),
        expert: l.has(ac::EXPERT),
        pause: l.has(ac::PAUSE),
        quote: l.has(ac::QUOTE),
        yes_no_bars: l.has(ac::YES_NO_BAR),
        deleted: l.has(ac::DELETED),
        palette: Palette(l.colors),
        page_length: l.page_length,
        start_menu: l.start_menu.clone(),
        total_calls: l.total_calls as u32,
        calls_today: l.calls_today as u32,
        first_call: parse_date(&l.first_call),
        last_call: parse_date(&l.last_call),
        time_today: l.time_today as u32,
        time_per_day: l.time_per_day as u32,
        posts: l.posts as u32,
        email: l.email as u32,
        uploads: l.uploads.max(0) as u32,
        downloads: l.downloads.max(0) as u32,
        upload_kb: l.upload_kb.max(0) as u64,
        download_kb: l.download_kb.max(0) as u64,
        file_points: l.file_points as u32,
        post_call: l.post_call as u32,
        limit_dl: l.limit_dl as u32,
        limit_dl_kb: l.limit_dl_kb as u32,
        today_dl: l.today_dl as u32,
        today_dl_kb: l.today_dl_kb as u32,
        uldl_ratio: l.uldl_ratio as u32,
        kb_ratio: l.kb_ratio as u32,
        cur_msg_area: l.cur_msg_area as u32,
        cur_file_area: l.cur_file_area as u32,
        conf_msg: l.conf_msg as u32,
        conf_file: l.conf_file as u32,
        autosig: l.autosig.clone(),
        vote_yes: l.vote_yes as u32,
        vote_no: l.vote_no as u32,
    };
    u.set_password(&l.password);
    u
}

/// Parse `import <src> [dest] [--force]` arguments.
pub fn from_args(args: &[String]) -> anyhow::Result<()> {
    let force = args.iter().any(|a| a == "--force");
    let paths: Vec<PathBuf> = args.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
    let (src, dest) = match paths.as_slice() {
        [src] => (src.clone(), PathBuf::from(".")),
        [src, dest] => (src.clone(), dest.clone()),
        _ => bail!("usage: iniquity import <old-install-dir> [dest-dir] [--force]"),
    };
    run(&src, &dest, force)
}
