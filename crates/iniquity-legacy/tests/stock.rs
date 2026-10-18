//! Checks against a stock r5 install: `original/stock` (unpacked by
//! `tools/setup_host.sh`) or `INIQUITY_STOCK`; skipped if neither exists.

use std::path::PathBuf;

use iniquity_legacy::*;

/// The stock install: `INIQUITY_STOCK` (relative paths are from the
/// workspace root), else `original/stock` if `tools/setup_host.sh` has
/// unpacked it. Without either, these tests do nothing.
fn stock() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = match std::env::var_os("INIQUITY_STOCK") {
        Some(p) => root.join(p),
        None => root.join("original/stock"),
    };
    dir.join("INIQUITY.DAT").exists().then_some(dir)
}

#[test]
fn config() {
    let Some(dir) = stock() else { return };
    let c = LegacyConfig::parse(&std::fs::read(dir.join("INIQUITY.DAT")).unwrap()).unwrap();
    assert_eq!(c.bbs_name, "Iniquity");
    assert_eq!(c.acs_sysop, "s255");
    assert_eq!(c.start_menu, "STARTUP");
    assert_eq!(c.bbs_location, "Somewhere, ??");
    assert_eq!(c.pmt_yes_word, "Yes");
    assert_eq!(c.inact_seconds, 180);
    assert!(c.matrix_login && c.multi_node);
    assert!(c.esc_to_exit);
    assert_eq!((c.blank_seconds, c.def_wfc_stat), (180, 8));
    assert_eq!((c.acs_post_email.as_str(), c.acs_anonymous.as_str()), ("s25", "s50"));
    assert_eq!(c.max_quote_lines, 11);
    assert!(c.ask_post_in_area && c.conf_ignore_msg && c.comp_msg_areas);
    assert!(c.ansi_string.starts_with("(ansi codes detected"));
    assert_eq!((c.kb_per_file_point, c.ul_search, c.file_points_per), (15, 4, 175));
    assert_eq!((c.file_desc1.as_str(), c.file_desc2.as_str()), ("FILE_ID.DIZ", "DESC.SDI"));
    assert_eq!(c.no_desc_line, "No description");
    assert_eq!((c.nuv_votes_yes, c.nuv_votes_no, c.nuv_user_level), (5, 3, 'B'));
    assert_eq!((c.nuv_access.as_str(), c.nuv_vote_access.as_str()), ("!s50", "s50"));
    assert!(c.nuv_validation && c.nuv_initials);
    assert!(!c.use_file_points && c.use_dl_limit && c.auto_validate && c.allow_blind && c.desc_wrap);
    assert_eq!(c.qwk_filename, "INIQUITY");
    assert_eq!((c.qwk_welcome.as_str(), c.qwk_news.as_str(), c.qwk_goodbye.as_str()), ("WELCOME.ANS", "NEWS.ANS", "LOGOFF.ANS"));
    assert!(c.qwk_strip_sigs);
    assert_eq!((c.chat_start.as_str(), c.chat_end.as_str(), c.chat_over_acs.as_str()), ("11:00", "20:00", "s150"));
    assert_eq!((c.max_page_times, c.max_page_beeps), (3, 6));
    assert_eq!((c.acs_attach_public.as_str(), c.acs_attach_email.as_str()), ("s50", "s50"));
    assert_eq!((c.age_limit, c.strict_age, c.del_list.as_str(), c.com_file.as_str()), (4, true, "DELETE.LST", "COMMENT.TXT"));
    assert!(c.virus_scan.starts_with("SCAN"));
    assert!(c.chat_page_noise && c.restore_chat_time && c.page_ask_email && c.no_chat_pw.is_empty());
    assert_eq!((c.light_char, c.light_char_ok, c.light_char_fail), ('■', '√', 'x'));
}

#[test]
fn small_files() {
    let Some(dir) = stock() else { return };
    let s = LegacyStats::parse(&std::fs::read(dir.join("DATA/STATS.DAT")).unwrap()).unwrap();
    assert!(s.first_day.len() == 8, "{s:?}");
    let h = LegacyHistory::parse_file(&std::fs::read(dir.join("DATA/HISTORY.DAT")).unwrap());
    assert_eq!(h.len(), 1);
    assert!(h[0].date.contains('/'), "{h:?}");
    let b = LegacyBbs::parse_file(&std::fs::read(dir.join("DATA/BBSLIST.DAT")).unwrap());
    assert_eq!(b.len(), 3);
    assert!(!b[0].name.is_empty(), "{b:?}");
    eprintln!("{s:?}\n{h:?}\n{b:#?}");
}

#[test]
fn file_areas() {
    let Some(dir) = stock() else { return };
    let areas = LegacyFileArea::parse_file(&std::fs::read(dir.join("DATA/FILEAREA.DAT")).unwrap()).unwrap();
    assert_eq!(areas.len(), 1);
    let a = &areas[0];
    assert_eq!((a.name.as_str(), a.filename.as_str(), a.sponsor.as_str()), ("New Iniquity file area", "NEWAREA", "SysOp"));
    assert!(a.path.ends_with("INSTALL\\"));
    assert!(a.sort_type <= 7);
}

#[test]
fn message_areas() {
    let Some(dir) = stock() else { return };
    let areas = LegacyMsgArea::parse_file(&std::fs::read(dir.join("DATA/MSGAREA.DAT")).unwrap()).unwrap();
    assert_eq!(areas.len(), 1);
    let a = &areas[0];
    assert_eq!((a.name.as_str(), a.filename.as_str(), a.acs.as_str(), a.post_acs.as_str()), ("New Iniquity Message Area", "NEWAREA", "s25", "s50"));
    assert_eq!((a.max_msgs, a.area_type), (200, 1));
    assert!(a.has(msgarea::flag::PRIVATE) && a.has(msgarea::flag::ANONYMOUS) && !a.has(msgarea::flag::MANDATORY));
    let email = LegacyMsgArea::parse_file(&std::fs::read(dir.join("DATA/EMAIL.DAT")).unwrap()).unwrap();
    assert_eq!((email[0].filename.as_str(), email[0].area_type), ("EMAIL", 0));
    let confs = LegacyConf::parse_file(&std::fs::read(dir.join("DATA/MSGCONF.DAT")).unwrap());
    assert_eq!((confs[0].desc.as_str(), confs[0].key), ("New Message Conference", 'X'));
}

#[test]
fn users() {
    let Some(dir) = stock() else { return };
    let users = LegacyUser::parse_file(&std::fs::read(dir.join("DATA/USERS.DAT")).unwrap()).unwrap();
    let u = &users[0];
    assert_eq!((u.number, u.user_name.as_str(), u.real_name.as_str()), (1, "SysOp", "System Operator"));
    assert_eq!((u.sl, u.dsl, u.level, u.page_length), (255, 255, 'E', 24));
    assert_eq!(u.last_call, "11/24/97");
    assert_eq!(u.first_call, "10/21/97");
    assert!(u.has(user::ac::ANSI) && u.has(user::ac::HOT_KEY) && !u.has(user::ac::DELETED));
}

#[test]
fn levels_and_strings() {
    let Some(dir) = stock() else { return };
    let levels = LegacyLevel::parse_file(&std::fs::read(dir.join("DATA/LEVELS.DAT")).unwrap()).unwrap();
    let a = levels.iter().find(|l| l.key == 'A').unwrap();
    assert_eq!((a.sl, a.desc.as_str()), (25, "New user access level"));
    let s = strings::parse_file(&std::fs::read(dir.join("DATA/STRINGS.DAT")).unwrap());
    assert_eq!(s.len(), 329);
    assert_eq!(strings::NAMES[8], "Pause");
    assert!(s[8].contains("Pa|02use"));
}

#[test]
fn menus() {
    let Some(dir) = stock() else { return };
    let m = LegacyMenu::parse(&std::fs::read(dir.join("MENU/MATRIX.MNU")).unwrap()).unwrap();
    assert_eq!((m.menu_type, m.title.as_str()), (3, "Login Matrix"));
    assert_eq!(m.commands.len(), 5);
    assert_eq!((m.commands[0].command.as_str(), m.commands[0].keys.as_str()), (">L", "1"));
    assert!(m.commands[0].on.starts_with("|B1"));
}
