use ratatui::style::{Color, Modifier, Style};

#[cfg(test)]
pub(crate) static COLOR_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn colors_active() -> bool {
    console::colors_enabled()
}

pub(crate) fn status_color(status: &str) -> Option<Color> {
    if !colors_active() {
        return None;
    }

    match status {
        "conflict" | "error" => Some(Color::Red),
        "stale" => Some(Color::Yellow),
        "fresh" => Some(Color::Green),
        "local" => Some(Color::Indexed(245)),
        _ => None,
    }
}

pub(crate) fn external_write_style() -> Style {
    if colors_active() {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

pub(crate) fn chrome_style() -> Style {
    if colors_active() {
        Style::default().fg(Color::Indexed(110))
    } else {
        Style::default()
    }
}

pub(crate) fn dim_style() -> Style {
    if colors_active() {
        Style::default().fg(Color::Indexed(245))
    } else {
        Style::default()
    }
}

pub(crate) fn selected_style() -> Style {
    let style = Style::default().add_modifier(Modifier::REVERSED);
    if colors_active() {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    struct ColorGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        prev: bool,
    }

    impl ColorGuard {
        fn set(enabled: bool) -> Self {
            let lock = COLOR_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let prev = console::colors_enabled();
            console::set_colors_enabled(enabled);
            Self { _lock: lock, prev }
        }
    }

    impl Drop for ColorGuard {
        fn drop(&mut self) {
            console::set_colors_enabled(self.prev);
        }
    }

    fn with_colors<T>(enabled: bool, f: impl FnOnce() -> T) -> T {
        let _guard = ColorGuard::set(enabled);
        f()
    }

    #[test]
    fn status_color_maps_traffic_light_semantics() {
        with_colors(true, || {
            assert_eq!(status_color("conflict"), Some(Color::Red));
            assert_eq!(status_color("error"), Some(Color::Red));
            assert_eq!(status_color("stale"), Some(Color::Yellow));
            assert_eq!(status_color("fresh"), Some(Color::Green));
            assert_eq!(status_color("local"), Some(Color::Indexed(245)));
            assert_eq!(status_color("unknown-state"), None);
        });
    }

    #[test]
    fn all_theme_styles_are_plain_when_colors_disabled() {
        with_colors(false, || {
            assert_eq!(status_color("conflict"), None);
            assert_eq!(external_write_style().fg, None);
            assert_eq!(chrome_style().fg, None);
            assert_eq!(dim_style().fg, None);
        });
    }

    #[test]
    fn external_write_style_is_red_bold_when_enabled() {
        with_colors(true, || {
            let style = external_write_style();
            assert_eq!(style.fg, Some(Color::Red));
            assert!(style.add_modifier.contains(ratatui::style::Modifier::BOLD));
        });
    }
}
