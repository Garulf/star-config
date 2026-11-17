//! `global.ini` strings and the game language.

use crate::staging::Document;
use crate::usercfg::UserCfg;
use std::collections::HashMap;
use std::path::Path;

const BOM: char = '\u{feff}';
const DEFAULT_LANGUAGE: &str = "english";
const LANGUAGE_KEY: &str = "g_language";
const PLURAL_SUFFIX: &str = ",P";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Strings {
    by_key: HashMap<String, String>,
}

impl Strings {
    pub fn parse(text: &str) -> Strings {
        let text = text.strip_prefix(BOM).unwrap_or(text);
        let by_key = text
            .lines()
            .filter_map(|line| line.split_once('='))
            .filter_map(|(key, value)| {
                let key = key.trim().trim_end_matches(PLURAL_SUFFIX);
                (!key.is_empty()).then(|| (key.to_lowercase(), value.to_string()))
            })
            .collect();
        Strings { by_key }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        let key = key.strip_prefix('@').unwrap_or(key);
        self.by_key.get(&key.to_lowercase()).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }
}

pub fn language(user_cfg_text: Option<&str>) -> String {
    user_cfg_text
        .and_then(|text| {
            let cfg = UserCfg::parse(Path::new("user.cfg"), text.as_bytes()).ok()?;
            let value = cfg.get(LANGUAGE_KEY)?.trim();
            (!value.is_empty()).then(|| value.to_string())
        })
        .unwrap_or_else(|| DEFAULT_LANGUAGE.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Strings {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/defaults/global.ini"
        );
        let bytes = std::fs::read(path).unwrap();
        Strings::parse(&String::from_utf8(bytes).unwrap())
    }

    #[test]
    fn strings_strip_bom_and_p_suffix_and_ignore_case() {
        let strings = fixture();
        assert_eq!(
            strings.get("@UI_CGSpaceFlightMovement"),
            Some("Flight movement")
        );
        assert_eq!(
            strings.get("ui_cgspaceflightmovement"),
            strings.get("@UI_CGSpaceFlightMovement")
        );
        assert_eq!(strings.get("ui_cipitchup"), Some("Pitch up (ship)"));
        assert_eq!(strings.get("missing"), None);
        assert_eq!(strings.get("@missing"), None);
    }

    #[test]
    fn the_last_duplicate_wins_and_values_keep_later_equals_signs() {
        let strings = fixture();
        assert_eq!(strings.get("ui_cieject"), Some("Eject from seat"));
        assert_eq!(strings.get("ui_equation"), Some("a=b"));
    }

    #[test]
    fn lines_without_a_pair_and_blank_lines_are_skipped() {
        let strings = fixture();
        assert_eq!(strings.len(), 10);
        assert!(!strings.is_empty());
        assert!(Strings::parse("").is_empty());
    }

    #[test]
    fn language_reads_g_language_or_defaults_to_english() {
        assert_eq!(
            language(Some("g_language = german_(germany)\n")),
            "german_(germany)"
        );
        assert_eq!(language(None), "english");
        assert_eq!(language(Some("r_width = 1920\n")), "english");
        assert_eq!(language(Some("g_language =\n")), "english");
    }
}
