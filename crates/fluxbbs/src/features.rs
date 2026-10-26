//! New: which parts of the board are on (`[features]`, the Features
//! page of `*C`). A part that's off answers its commands with
//! `FeatureOff`, and menus leave its entries out when they load, so the
//! lightbars, the generic lists, the command help and the logon
//! sequence all skip it. Parts with a switch of their own (AI chat,
//! BBSLink, new user voting) count here too, by that switch.

use crate::config::Config;

/// The feature a command belongs to, if it belongs to one: a name from
/// `[features]`, or `ai`, `bbslink`, `nuv` for the parts switched
/// elsewhere.
pub fn feature_of(code: &str) -> Option<&'static str> {
    Some(match code.to_ascii_uppercase().as_str() {
        "AT" => "tarot",
        "AL" => "slots",
        "AC" => "casino",
        "AP" => "carrier_lost",
        "AG" => "gallery",
        "MH" | "MX" => "message_center",
        "!D" | "!W" | "!E" | "!I" | "!V" => "time_bank",
        "VV" | "VA" | "VR" | "VN" | "VD" => "voting",
        "BA" | "BL" | "BS" | "BD" | "BC" => "bbs_list",
        "IN" | "IM" => "infoforms",
        "QD" | "QU" => "qwk",
        "AS" | "AW" | "AR" => "automessage",
        "-X" => "scripts",
        "AI" => "ai",
        "DB" => "bbslink",
        "NV" => "nuv",
        _ => return None,
    })
}

/// Is the feature a command belongs to on? Commands that belong to
/// none always are.
pub fn command_on(cfg: &Config, code: &str) -> bool {
    match feature_of(code) {
        None => true,
        Some("ai") => cfg.ai.enabled,
        Some("bbslink") => cfg.bbslink.enabled,
        Some("nuv") => cfg.nuv.enabled,
        Some(name) => cfg.features.on(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switched_off_features_refuse_their_commands() {
        let mut cfg = Config::default();
        assert!(command_on(&cfg, "AT") && command_on(&cfg, "MA") && command_on(&cfg, "at"));
        cfg.features.tarot = false;
        cfg.features.time_bank = false;
        assert!(!command_on(&cfg, "AT") && !command_on(&cfg, "!W"));
        assert!(command_on(&cfg, "AC") && command_on(&cfg, "MA"), "the rest still answer");
        cfg.ai.enabled = false;
        assert!(!command_on(&cfg, "AI"), "AI chat goes by its own switch");
        assert_eq!(cfg.features.off().len(), 2);
    }
}
