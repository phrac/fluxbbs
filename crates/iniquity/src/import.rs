//! `iniquity import <old install> [dest]`: convert an Iniquity 1.00a26
//! (DOS) installation into this port's files.
//!
//! | from                  | to                          |
//! |-----------------------|-----------------------------|
//! | INIQUITY.DAT          | iniquity.toml               |
//! | DATA\LEVELS.DAT       | levels.toml                 |
//! | DATA\STRINGS.DAT      | strings.toml                |
//! | DATA\USERS.DAT        | data/iniquity.db            |
//! | DATA\MSGAREA.DAT etc. | msgareas.toml               |
//! | MSGS\*.HDR, *.DAT     | data/iniquity.db            |
//! | DATA\FILEAREA.DAT etc.| fileareas.toml              |
//! | DATA\*.DIR, FILEDESC  | data/iniquity.db            |
//! | MENU\*.MNU            | menus/*.toml                |
//! | TEXT\*                | text/                       |

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use iniquity_legacy::msgarea::{flag, status};
use iniquity_legacy::{LegacyBbs, LegacyCall, LegacyInfoform, LegacyTextLib, LegacyConf, LegacyConfig, LegacyFile, LegacyFileArea, LegacyHistory, LegacyStats, LegacyLevel, LegacyMenu, LegacyMsgArea, LegacyMsgHeader, LegacyUser, strings, user::ac};
use iniquity_term::Palette;

use crate::config::{self, Config, Level};
use crate::menu::{Command, Menu, MenuType, Mode};
use crate::fileareas::{FileArea, FileAreas, SortBy};
use crate::filebase::{FileBase, FileRec};
use crate::msgareas::{Conf, MsgArea, MsgAreas};
use crate::msgbase::{Message, MsgBase, Party};
use crate::sysdata::{Bbs, Call, Counts, SysData};
use crate::infoform::{FormStore, Infoform, Infoforms};
use crate::themes::{Theme, Themes};
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
    // Levels get a file of their own, which the level editor rewrites.
    let levels = crate::extprog::Levels(std::mem::take(&mut cfg.levels));
    std::fs::write(&cfg_path, header + &toml::to_string_pretty(&cfg)?)?;
    println!("wrote {}", cfg_path.display());
    let levels_path = dest.join(&cfg.paths.levels);
    std::fs::write(&levels_path, levels.to_toml())?;
    println!("wrote {} ({} levels)", levels_path.display(), levels.0.values().filter(|l| !l.desc.is_empty()).count());

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
    let mut user_slots = 0u16;
    // (user number, text library number) for after the libraries.
    let mut user_libs: Vec<(u32, u8)> = Vec::new();
    if let Some(p) = find_ci(&data, "USERS.DAT") {
        for (i, lu) in LegacyUser::parse_file(&std::fs::read(p)?)?.iter().enumerate() {
            user_slots = i as u16 + 1;
            if lu.user_name.trim().is_empty() {
                continue;
            }
            let mut u = convert_user(lu);
            u.id = i as u32 + 1;
            user_libs.push((u.id, lu.text_lib));
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

    // File conferences, areas and records. The files themselves stay where
    // they are: DOS paths can't be carried over.
    let mut fareas = FileAreas::default();
    if let Some(buf) = read("FILECONF.DAT")? {
        let confs: Vec<Conf> =
            LegacyConf::parse_file(&buf).into_iter().map(|c| Conf { key: c.key, desc: c.desc, acs: c.acs }).collect();
        if !confs.is_empty() {
            fareas.confs = confs;
        }
    }
    let legacy_areas = match read("FILEAREA.DAT")? {
        Some(buf) => LegacyFileArea::parse_file(&buf)?,
        None => Vec::new(),
    };
    if !legacy_areas.is_empty() {
        fareas.areas = legacy_areas
            .iter()
            .map(|a| FileArea {
                name: a.name.clone(),
                file: a.filename.to_ascii_uppercase(),
                path: format!("files/{}", a.filename.to_ascii_lowercase()).into(),
                sponsor: a.sponsor.clone(),
                acs: a.acs.clone(),
                upload_acs: a.acs_upload.clone(),
                download_acs: a.acs_download.clone(),
                password: a.password.clone(),
                sort: SortBy::from_legacy(a.sort_type),
                sort_descending: !a.sort_ascending,
            })
            .collect();
    }
    let out = dest.join("fileareas.toml");
    std::fs::write(&out, fareas.to_toml())?;
    println!("wrote {} ({} areas, {} conferences)", out.display(), fareas.areas.len(), fareas.confs.len());
    // Statistics, history, today's callers and the BBS list.
    let sys = SysData::open(&db_path)?;
    let mut history_sum = Counts::default();
    if let Some(buf) = read("HISTORY.DAT")? {
        for h in LegacyHistory::parse_file(&buf) {
            let Some(day) = parse_date(&h.date) else { continue };
            let c = Counts {
                calls: h.calls as u64,
                new_users: h.new_users as u64,
                posts: h.posts as u64,
                email: h.email as u64,
                uploads: h.uploads as u64,
                downloads: h.downloads as u64,
                upload_kb: h.upload_kb as u64,
                download_kb: h.download_kb as u64,
            };
            history_sum.calls += c.calls;
            history_sum.posts += c.posts;
            history_sum.email += c.email;
            history_sum.uploads += c.uploads;
            history_sum.downloads += c.downloads;
            history_sum.upload_kb += c.upload_kb;
            history_sum.download_kb += c.download_kb;
            sys.set_day(day, &c)?;
        }
    }
    if let Some(buf) = read("STATS.DAT")? {
        let s = LegacyStats::parse(&buf)?;
        let less = |total: u64, part: u64| total.saturating_sub(part);
        sys.set_base(
            parse_date(&s.first_day),
            &Counts {
                calls: less(s.calls as u64, history_sum.calls),
                posts: less(s.posts as u64, history_sum.posts),
                email: less(s.email as u64, history_sum.email),
                uploads: less(s.uploads.max(0) as u64, history_sum.uploads),
                downloads: less(s.downloads.max(0) as u64, history_sum.downloads),
                upload_kb: less(s.upload_kb.max(0) as u64, history_sum.upload_kb),
                download_kb: less(s.download_kb.max(0) as u64, history_sum.download_kb),
                new_users: 0,
            },
        )?;
    }
    if let Some(buf) = read("CALLERS.DAT")? {
        for c in LegacyCall::parse_file(&buf) {
            let at = parse_date(&c.date).and_then(|d| {
                let t = chrono::NaiveTime::parse_from_str(&c.time.replace("am", " AM").replace("pm", " PM"), "%l:%M %p").ok()?;
                Some(d.and_time(t))
            });
            sys.add_call(&Call {
                call_num: c.call_num.max(0) as u64,
                user_id: c.user_num as u32,
                handle: c.user_name.clone(),
                location: c.location.clone(),
                at: at.unwrap_or_default(),
                new_user: c.new_user,
                area_code: c.area_code.clone(),
                transport: "modem".into(),
            })?;
        }
    }
    let mut nbbs = 0;
    if let Some(buf) = read("BBSLIST.DAT")? {
        for b in LegacyBbs::parse_file(&buf) {
            sys.add_bbs(&Bbs {
                id: 0,
                name: b.name,
                address: b.phone,
                sysop: b.sysop,
                speed: b.baud.max(0) as u32,
                software: b.software,
                storage: b.storage,
                info: b.info,
                who_added: b.who_added,
            })?;
            nbbs += 1;
        }
    }
    println!("imported statistics, history and {nbbs} BBS list entries");

    // Infoforms (INFOFORM.DAT), everyone's answers (INFOFORM.001...) and
    // new user votes (NUV.DAT). User numbers are kept, so they line up.
    let forms = FormStore::open(&db_path)?;
    let now = chrono::Local::now().naive_local();
    let legacy_forms = read("INFOFORM.DAT")?.map(|b| LegacyInfoform::parse_file(&b)).unwrap_or_default();
    let mut nanswers = 0;
    let mut list = Vec::new();
    for (i, f) in legacy_forms.iter().enumerate() {
        let file = f.filename.to_ascii_uppercase();
        if let Some(buf) = read(&format!("INFOFORM.{:03}", i + 1))? {
            for user in 1..=user_slots {
                if let Some(answers) = iniquity_legacy::misc::infoform_answers(&buf, user) {
                    forms.set_answers(&file, user as u32, &answers, now)?;
                    nanswers += 1;
                }
            }
        }
        list.push(Infoform { desc: f.desc.clone(), file, mandatory: f.mandatory, nuv: f.nuv, acs: f.acs.clone() });
    }
    if !list.is_empty() {
        let out = dest.join("infoforms.toml");
        std::fs::write(&out, Infoforms { list }.to_toml())?;
        println!("wrote {} ({} forms, {nanswers} filled in)", out.display(), legacy_forms.len());
    }
    let mut nvotes = 0;
    if let Some(buf) = read("NUV.DAT")? {
        for v in iniquity_legacy::misc::parse_votes(&buf) {
            forms.add_vote(v.voter as u32, v.candidate as u32, &v.initials, &v.comment, now)?;
            nvotes += 1;
        }
    }
    if nvotes > 0 {
        println!("imported {nvotes} new user votes");
    }

    // Text libraries (TEXTLIB.DAT and the .TFL files it names) become
    // screen themes: a directory each, plus themes.toml.
    let lib_names = read("TEXTLIB.DAT")?.map(|b| iniquity_legacy::misc::text_lib_names(&b)).unwrap_or_default();
    let mut themes = Themes::default();
    for name in &lib_names {
        let file = format!("{name}.TFL");
        let Some(path) = find_tfl(src, &file) else {
            println!("text library {file} not found; skipped");
            continue;
        };
        let lib = LegacyTextLib::parse(&std::fs::read(&path)?)?;
        let dir_name = name.to_ascii_lowercase();
        let dir = dest.join("themes").join(&dir_name);
        std::fs::create_dir_all(&dir)?;
        for (fname, data) in &lib.files {
            std::fs::write(dir.join(fname.to_ascii_uppercase()), data)?;
        }
        println!("unpacked text library {file} into {} ({} files)", dir.display(), lib.files.len());
        themes.list.push(Theme { dir: dir_name, desc: lib.desc, author: lib.author });
    }
    if !themes.list.is_empty() {
        std::fs::write(dest.join("themes.toml"), themes.to_toml())?;
        // Users keep the library they had (the first is the default).
        for (id, n) in user_libs {
            let Some(t) = (n as usize).checked_sub(1).and_then(|i| themes.list.get(i)).filter(|_| n > 1) else { continue };
            if let Some(mut u) = store.get(id)? {
                u.theme = t.dir.clone();
                store.save(&u)?;
            }
        }
    }

    let fbase = FileBase::open(&db_path)?;
    let descs = read("FILEDESC.DAT")?.unwrap_or_default();
    for (area, legacy) in fareas.areas.iter().zip(&legacy_areas) {
        std::fs::create_dir_all(dest.join(&area.path))?;
        let Some(buf) = read(&format!("{}.DIR", area.file))? else { continue };
        let files = LegacyFile::parse_file(&buf)?;
        for (i, f) in files.iter().enumerate() {
            let mut rec = FileRec {
                area: area.file.clone(),
                num: i as u32 + 1,
                name: f.filename.clone(),
                size: f.size.max(0) as u64,
                date: parse_date(&f.date),
                uploader: f.uploader.clone(),
                uploaded: parse_date(&f.ul_date),
                downloads: f.downloads as u32,
                points: f.file_points as u32,
                valid: f.valid,
                description: f.description(&descs),
            };
            fbase.add(&mut rec)?;
        }
        if !files.is_empty() {
            println!(
                "imported {} file records for {}; copy the files from {} into {}",
                files.len(),
                area.name,
                legacy.path,
                dest.join(&area.path).display()
            );
        }
    }
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
    p.light = c.light_char;
    p.light_ok = c.light_char_ok;
    p.light_fail = c.light_char_fail;
    cfg.qwk = crate::config::Qwk {
        packet: c.qwk_filename.clone(),
        welcome: c.qwk_welcome.clone(),
        news: c.qwk_news.clone(),
        goodbye: c.qwk_goodbye.clone(),
        strip_sigs: c.qwk_strip_sigs,
    };
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
    m.acs_anon_automsg = c.acs_anon_automsg.clone();
    cfg.logon.last_callers = c.num_last_calls as u32;
    let f = &mut cfg.files;
    f.use_file_points = c.use_file_points;
    f.kb_per_point = c.kb_per_file_point;
    f.upload_points_percent = c.file_points_per;
    f.use_daily_limit = c.use_dl_limit;
    f.use_daily_kb_limit = c.use_dl_kb_limit;
    f.use_ratio = c.use_uldl_ratio;
    f.use_kb_ratio = c.use_kb_ratio;
    f.auto_validate = c.auto_validate;
    f.allow_blind_upload = c.allow_blind;
    f.upload_dupe_check = c.ul_search;
    f.import_descriptions = c.import_descs;
    f.description_files = [&c.file_desc1, &c.file_desc2].into_iter().filter(|s| !s.is_empty()).cloned().collect();
    f.no_description = c.no_desc_line.clone();
    f.wrap_descriptions = c.desc_wrap;
    f.advance_bar_on_flag = c.adv_file_bar;
    cfg.nuv = crate::config::Nuv {
        enabled: c.nuv_validation,
        votes_yes: c.nuv_votes_yes,
        votes_no: c.nuv_votes_no,
        candidate_acs: c.nuv_access.clone(),
        voter_acs: c.nuv_vote_access.clone(),
        initials: c.nuv_initials,
        level: c.nuv_user_level.to_ascii_uppercase(),
    };
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
                    // LEVELS.DAT in this release has no time bank.
                    ..Level::default()
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
        max_msgs: a.max_msgs as u32,
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
        file_scan: parse_date(&l.file_scan),
        // Set from TEXTLIB.DAT once the libraries are imported.
        theme: String::new(),
        last_rep: String::new(),
        bank_time: 0,
        bank_kb: 0,
        bank_time_in: 0,
        bank_time_out: 0,
        bank_kb_in: 0,
        bank_kb_out: 0,
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

/// A library file: in the install's top directory or one level down
/// (the stock path was a directory of its own).
fn find_tfl(src: &Path, file: &str) -> Option<std::path::PathBuf> {
    find_ci(src, file).or_else(|| {
        std::fs::read_dir(src).ok()?.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).find_map(|d| find_ci(&d, file))
    })
}
