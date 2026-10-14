use emojis::SkinTone;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Slack,
    Emojisense,
    Teams,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub styles: Vec<Style>,
    pub skin_tone: SkinTone,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            styles: vec![Style::Slack],
            skin_tone: SkinTone::Default,
        }
    }
}

impl Settings {
    /// Reads the settings Zed sends, using the default for any value that is missing or unknown.
    pub fn from_json(value: &Value) -> Settings {
        Settings {
            styles: styles_setting(value.get("styles")),
            skin_tone: skin_tone_setting(value.get("skin_tone")),
        }
    }
}

fn styles_setting(value: Option<&Value>) -> Vec<Style> {
    let Some(value) = value else {
        return Settings::default().styles;
    };
    let Some(entries) = value.as_array() else {
        eprintln!("ignoring styles {value}, which is not a list");
        return Settings::default().styles;
    };
    entries
        .iter()
        .filter_map(|entry| match entry.as_str() {
            Some("slack") => Some(Style::Slack),
            Some("emojisense") => Some(Style::Emojisense),
            Some("teams") => Some(Style::Teams),
            _ => {
                eprintln!("ignoring unknown style {entry}");
                None
            }
        })
        .collect()
}

fn skin_tone_setting(value: Option<&Value>) -> SkinTone {
    let Some(value) = value else {
        return SkinTone::Default;
    };
    match value.as_str() {
        Some("default") => SkinTone::Default,
        Some("light") => SkinTone::Light,
        Some("medium-light") => SkinTone::MediumLight,
        Some("medium") => SkinTone::Medium,
        Some("medium-dark") => SkinTone::MediumDark,
        Some("dark") => SkinTone::Dark,
        _ => {
            eprintln!("ignoring unknown skin_tone {value}");
            SkinTone::Default
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_settings_are_default() {
        assert_eq!(Settings::from_json(&Value::Null), Settings::default());
        assert_eq!(
            Settings::from_json(&serde_json::json!({})),
            Settings::default()
        );
        assert_eq!(Settings::default().styles, vec![Style::Slack]);
        assert_eq!(Settings::default().skin_tone, SkinTone::Default);
    }

    #[test]
    fn reads_styles() {
        let settings =
            Settings::from_json(&serde_json::json!({ "styles": ["emojisense", "teams"] }));
        assert_eq!(settings.styles, vec![Style::Emojisense, Style::Teams]);
    }

    #[test]
    fn empty_styles_turn_completions_off() {
        let settings = Settings::from_json(&serde_json::json!({ "styles": [] }));
        assert!(settings.styles.is_empty());
    }

    #[test]
    fn unknown_styles_are_ignored() {
        let settings = Settings::from_json(&serde_json::json!({ "styles": ["teams", "irc", 3] }));
        assert_eq!(settings.styles, vec![Style::Teams]);
    }

    #[test]
    fn styles_that_are_not_a_list_are_default() {
        let settings = Settings::from_json(&serde_json::json!({ "styles": "teams" }));
        assert_eq!(settings.styles, vec![Style::Slack]);
    }

    #[test]
    fn reads_each_skin_tone() {
        let cases = [
            ("default", SkinTone::Default),
            ("light", SkinTone::Light),
            ("medium-light", SkinTone::MediumLight),
            ("medium", SkinTone::Medium),
            ("medium-dark", SkinTone::MediumDark),
            ("dark", SkinTone::Dark),
        ];
        for (name, tone) in cases {
            let settings = Settings::from_json(&serde_json::json!({ "skin_tone": name }));
            assert_eq!(settings.skin_tone, tone, "{name}");
        }
    }

    #[test]
    fn unknown_skin_tone_is_default() {
        let settings = Settings::from_json(&serde_json::json!({ "skin_tone": "purple" }));
        assert_eq!(settings.skin_tone, SkinTone::Default);
        let settings = Settings::from_json(&serde_json::json!({ "skin_tone": 3 }));
        assert_eq!(settings.skin_tone, SkinTone::Default);
    }
}
