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
        // The temple, retired: old menus drop it.
        "AD" => "temple",
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
        "D0" | "D1" | "D2" | "D3" | "D4" | "D7" | "DZ" => "doors",
        "NV" => "nuv",
        "UV" => "email",
        "WX" => "weather",
        "XR" => "mrc",
        _ => return None,
    })
}

/// New: was this part built into the board at all? The optional
/// components are Cargo features (docs/BUILDING.md); one left out
/// counts as switched off, whatever `[features]` says.
pub fn built(name: &str) -> bool {
    const BUILT: [(&str, bool); 12] = [
        ("mrc", cfg!(feature = "mrc")),
        ("weather", cfg!(feature = "weather")),
        ("email", cfg!(feature = "email")),
        ("temple", false),
        ("tarot", cfg!(feature = "tarot")),
        ("slots", cfg!(feature = "casino")),
        ("casino", cfg!(feature = "casino")),
        ("carrier_lost", cfg!(feature = "carrierlost")),
        ("ai", cfg!(feature = "ai")),
        ("bbslink", cfg!(feature = "doors")),
        ("doors", cfg!(feature = "doors")),
        ("scripts", cfg!(feature = "lua")),
    ];
    BUILT.iter().find(|(n, _)| *n == name).is_none_or(|(_, on)| *on)
}

/// Is the feature a command belongs to on? Commands that belong to
/// none always are.
pub fn command_on(cfg: &Config, code: &str) -> bool {
    match feature_of(code) {
        None => true,
        Some(name) if !built(name) => false,
        Some("ai") => cfg.ai.enabled,
        Some("bbslink") => cfg.bbslink.enabled,
        Some("nuv") => cfg.nuv.enabled,
        Some("email") => cfg.email.enabled,
        Some("mrc") => cfg.mrc.enabled,
        Some(name) => cfg.features.on(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switched_off_features_refuse_their_commands() {
        let mut cfg = Config::default();
        assert!(command_on(&cfg, "!W") && command_on(&cfg, "MA") && command_on(&cfg, "!w"));
        cfg.features.time_bank = false;
        cfg.features.voting = false;
        assert!(!command_on(&cfg, "!W") && !command_on(&cfg, "VV"));
        assert!(command_on(&cfg, "BL") && command_on(&cfg, "MA"), "the rest still answer");
        cfg.ai.enabled = false;
        assert!(!command_on(&cfg, "AI"), "AI chat goes by its own switch");
        assert_eq!(cfg.features.off().len(), 2);
    }

    #[test]
    fn parts_left_out_of_the_build_are_off() {
        let cfg = Config::default();
        assert_eq!(command_on(&cfg, "AT"), cfg!(feature = "tarot"));
        assert_eq!(command_on(&cfg, "AC"), cfg!(feature = "casino"));
        assert_eq!(command_on(&cfg, "AP"), cfg!(feature = "carrierlost"));
        assert_eq!(command_on(&cfg, "D1"), cfg!(feature = "doors"));
        assert_eq!(command_on(&cfg, "-X"), cfg!(feature = "lua"));
        assert!(built("message_center") && built("anything else"), "the core is always built");
        assert!(!command_on(&cfg, "AD"), "the retired temple never is");
    }
}
