//! Human-output localization for `wt`.
//!
//! `wt` keeps a single source-of-truth English catalog and resolves an effective
//! [`Lang`] from the configured [`crate::config::Language`] plus the OS locale.
//! Untranslated entries fall back to English. Machine-readable output (`--json`)
//! always renders English so it stays reproducible across locales; that switch
//! lives in [`crate::context::Ctx::lang`].

use crate::config::Language;

/// The effective language an output string is rendered in. Unlike
/// [`Language`], there is no `Auto` here — it has already been resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ko,
}

impl Lang {
    /// Resolve the effective language from the configured value and an optional
    /// detected OS locale string (e.g. `"ko_KR.UTF-8"`).
    ///
    /// `Auto` matches the locale by prefix: a `ko*` locale resolves to Korean,
    /// anything else (including an absent/garbage locale) resolves to English.
    pub fn resolve(language: Language, detected_locale: Option<&str>) -> Lang {
        match language {
            Language::En => Lang::En,
            Language::Ko => Lang::Ko,
            Language::Auto => match detected_locale {
                Some(locale) if locale.to_ascii_lowercase().starts_with("ko") => Lang::Ko,
                _ => Lang::En,
            },
        }
    }

    /// Read the OS locale from the environment, preferring `LC_ALL`, then
    /// `LC_MESSAGES`, then `LANG`. Empty values are treated as unset.
    pub fn detect_locale() -> Option<String> {
        for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(value) = std::env::var(key)
                && !value.is_empty()
            {
                return Some(value);
            }
        }
        None
    }
}

/// Dep-free interpolation: replace each `{name}` placeholder in `template` with
/// its value. Slot values flow through verbatim and are never translated, so
/// paths, command literals, and error text stay as-is inside localized prose.
pub fn fill(template: &str, slots: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in slots {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_language_ignores_locale() {
        assert_eq!(Lang::resolve(Language::En, Some("ko_KR.UTF-8")), Lang::En);
        assert_eq!(Lang::resolve(Language::Ko, Some("en_US.UTF-8")), Lang::Ko);
    }

    #[test]
    fn auto_matches_korean_locale_by_prefix() {
        assert_eq!(Lang::resolve(Language::Auto, Some("ko_KR.UTF-8")), Lang::Ko);
        assert_eq!(Lang::resolve(Language::Auto, Some("ko")), Lang::Ko);
    }

    #[test]
    fn auto_falls_back_to_english_for_other_or_missing_locale() {
        assert_eq!(Lang::resolve(Language::Auto, Some("en_US.UTF-8")), Lang::En);
        assert_eq!(Lang::resolve(Language::Auto, Some("")), Lang::En);
        assert_eq!(Lang::resolve(Language::Auto, None), Lang::En);
        assert_eq!(Lang::resolve(Language::Auto, Some("garbage")), Lang::En);
    }

    #[test]
    fn fill_replaces_named_slots_and_leaves_values_verbatim() {
        let out = fill(
            "{path} 에 {line} 가 없습니다. {hint}",
            &[
                ("path", "~/.zshrc"),
                ("line", "`eval`"),
                ("hint", "Run wt setup."),
            ],
        );
        assert_eq!(out, "~/.zshrc 에 `eval` 가 없습니다. Run wt setup.");
    }

    #[test]
    fn fill_without_slots_is_identity() {
        assert_eq!(fill("no slots here", &[]), "no slots here");
    }
}
