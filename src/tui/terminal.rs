use anyhow::{Context, Result, bail};
use ratatui::crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

static INSTALL_PANIC_HOOK: Once = Once::new();
static CROSSTERM_SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

pub(crate) trait TerminalEffects {
    fn enter(&self) -> Result<()>;
    fn set_mouse_capture(&self, enabled: bool) -> Result<()>;
    fn leave(&self) -> Result<()>;
}

pub(crate) struct CrosstermEffects;

impl TerminalEffects for CrosstermEffects {
    fn enter(&self) -> Result<()> {
        enter_crossterm_terminal(&RealCrosstermSideEffects)
    }

    fn set_mouse_capture(&self, enabled: bool) -> Result<()> {
        set_crossterm_mouse_capture(&RealCrosstermSideEffects, enabled)
    }

    fn leave(&self) -> Result<()> {
        leave_crossterm_terminal(&RealCrosstermSideEffects)
    }
}

trait CrosstermSideEffects {
    fn enable_raw_mode(&self) -> Result<()>;
    fn enter_alternate_screen(&self) -> Result<()>;
    fn enable_mouse_capture(&self) -> Result<()>;
    fn disable_mouse_capture(&self) -> Result<()>;
    fn leave_alternate_screen(&self) -> Result<()>;
    fn disable_raw_mode(&self) -> Result<()>;
}

struct RealCrosstermSideEffects;

impl CrosstermSideEffects for RealCrosstermSideEffects {
    fn enable_raw_mode(&self) -> Result<()> {
        terminal::enable_raw_mode().context("enable terminal raw mode")
    }

    fn enter_alternate_screen(&self) -> Result<()> {
        execute!(io::stdout(), EnterAlternateScreen).context("enter terminal alternate screen")
    }

    fn enable_mouse_capture(&self) -> Result<()> {
        execute!(io::stdout(), EnableMouseCapture).context("enable terminal mouse capture")
    }

    fn disable_mouse_capture(&self) -> Result<()> {
        execute!(io::stdout(), DisableMouseCapture).context("disable terminal mouse capture")
    }

    fn leave_alternate_screen(&self) -> Result<()> {
        execute!(io::stdout(), LeaveAlternateScreen).context("leave terminal alternate screen")
    }

    fn disable_raw_mode(&self) -> Result<()> {
        terminal::disable_raw_mode().context("disable terminal raw mode")
    }
}

fn enter_crossterm_terminal(side_effects: &impl CrosstermSideEffects) -> Result<()> {
    if CROSSTERM_SESSION_ACTIVE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        bail!("a terminal session is already active");
    }

    if let Err(err) = side_effects.enable_raw_mode() {
        CROSSTERM_SESSION_ACTIVE.store(false, Ordering::SeqCst);
        return Err(err);
    }

    if let Err(err) = side_effects.enter_alternate_screen() {
        let _ = side_effects.disable_raw_mode();
        CROSSTERM_SESSION_ACTIVE.store(false, Ordering::SeqCst);
        return Err(err);
    }

    if let Err(err) = side_effects.enable_mouse_capture() {
        let _ = side_effects.leave_alternate_screen();
        let _ = side_effects.disable_raw_mode();
        CROSSTERM_SESSION_ACTIVE.store(false, Ordering::SeqCst);
        return Err(err);
    }

    Ok(())
}

fn set_crossterm_mouse_capture(
    side_effects: &impl CrosstermSideEffects,
    enabled: bool,
) -> Result<()> {
    if enabled {
        side_effects.enable_mouse_capture()
    } else {
        side_effects.disable_mouse_capture()
    }
}

fn leave_crossterm_terminal(side_effects: &impl CrosstermSideEffects) -> Result<()> {
    if !CROSSTERM_SESSION_ACTIVE.load(Ordering::SeqCst) {
        return Ok(());
    }

    let mouse_result = side_effects.disable_mouse_capture();
    let screen_result = side_effects.leave_alternate_screen();
    let raw_mode_result = side_effects.disable_raw_mode();
    CROSSTERM_SESSION_ACTIVE.store(false, Ordering::SeqCst);

    for result in [mouse_result, screen_result, raw_mode_result] {
        result?;
    }
    Ok(())
}

pub(crate) struct TerminalSession<E: TerminalEffects = CrosstermEffects> {
    effects: E,
    active: bool,
    mouse_capture_enabled: bool,
}

impl TerminalSession<CrosstermEffects> {
    pub(crate) fn new() -> Result<Self> {
        install_panic_hook_once();
        Self::with_effects(CrosstermEffects)
    }
}

impl<E: TerminalEffects> TerminalSession<E> {
    pub(crate) fn with_effects(effects: E) -> Result<Self> {
        effects.enter()?;
        Ok(Self {
            effects,
            active: true,
            mouse_capture_enabled: true,
        })
    }

    pub(crate) fn set_mouse_capture(&mut self, enabled: bool) -> Result<()> {
        if self.mouse_capture_enabled == enabled {
            return Ok(());
        }
        self.effects.set_mouse_capture(enabled)?;
        self.mouse_capture_enabled = enabled;
        Ok(())
    }
}

impl<E: TerminalEffects> Drop for TerminalSession<E> {
    fn drop(&mut self) {
        if self.active {
            if self.mouse_capture_enabled {
                let _ = self.effects.set_mouse_capture(false);
                self.mouse_capture_enabled = false;
            }
            let _ = self.effects.leave();
            self.active = false;
        }
    }
}

fn install_panic_hook_once() {
    INSTALL_PANIC_HOOK.call_once(|| {
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            restore_terminal_best_effort();
            previous_hook(panic_info);
        }));
    });
}

fn restore_terminal_best_effort() {
    let effects = CrosstermEffects;
    let _ = effects.leave();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    static CROSSTERM_ACTIVE_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[derive(Default)]
    struct RecordingEffects {
        log: Arc<Mutex<Vec<&'static str>>>,
    }

    #[derive(Default)]
    struct RecordingCrosstermSideEffects {
        log: Arc<Mutex<Vec<&'static str>>>,
    }

    impl CrosstermSideEffects for RecordingCrosstermSideEffects {
        fn enable_raw_mode(&self) -> anyhow::Result<()> {
            self.log.lock().unwrap().push("enable_raw_mode");
            Ok(())
        }

        fn enter_alternate_screen(&self) -> anyhow::Result<()> {
            self.log.lock().unwrap().push("enter_alternate_screen");
            Ok(())
        }

        fn enable_mouse_capture(&self) -> anyhow::Result<()> {
            self.log.lock().unwrap().push("enable_mouse_capture");
            Ok(())
        }

        fn disable_mouse_capture(&self) -> anyhow::Result<()> {
            self.log.lock().unwrap().push("disable_mouse_capture");
            Ok(())
        }

        fn leave_alternate_screen(&self) -> anyhow::Result<()> {
            self.log.lock().unwrap().push("leave_alternate_screen");
            Ok(())
        }

        fn disable_raw_mode(&self) -> anyhow::Result<()> {
            self.log.lock().unwrap().push("disable_raw_mode");
            Ok(())
        }
    }

    struct ResetCrosstermSessionActive;

    impl Drop for ResetCrosstermSessionActive {
        fn drop(&mut self) {
            CROSSTERM_SESSION_ACTIVE.store(false, Ordering::SeqCst);
        }
    }

    impl TerminalEffects for RecordingEffects {
        fn enter(&self) -> anyhow::Result<()> {
            self.log.lock().unwrap().push("enter");
            Ok(())
        }

        fn set_mouse_capture(&self, enabled: bool) -> anyhow::Result<()> {
            self.log.lock().unwrap().push(if enabled {
                "enable_mouse_capture"
            } else {
                "disable_mouse_capture"
            });
            Ok(())
        }

        fn leave(&self) -> anyhow::Result<()> {
            self.log.lock().unwrap().push("leave");
            Ok(())
        }
    }

    #[test]
    fn session_enters_on_create_and_leaves_on_drop() {
        let effects = RecordingEffects::default();
        let log = Arc::clone(&effects.log);
        {
            let _session = TerminalSession::with_effects(effects).unwrap();
            assert_eq!(*log.lock().unwrap(), vec!["enter"]);
        }
        assert_eq!(
            *log.lock().unwrap(),
            vec!["enter", "disable_mouse_capture", "leave"]
        );
    }

    #[test]
    fn session_drop_disables_mouse_capture_before_leave() {
        let effects = RecordingEffects::default();
        let log = Arc::clone(&effects.log);

        let session = TerminalSession::with_effects(effects).unwrap();
        drop(session);

        assert_eq!(
            *log.lock().unwrap(),
            vec!["enter", "disable_mouse_capture", "leave"]
        );
    }

    #[test]
    fn crossterm_enter_rejects_nested_session_without_side_effects() {
        let _guard = CROSSTERM_ACTIVE_TEST_LOCK.lock().unwrap();
        CROSSTERM_SESSION_ACTIVE.store(true, Ordering::SeqCst);
        let _reset = ResetCrosstermSessionActive;
        let side_effects = RecordingCrosstermSideEffects::default();
        let log = Arc::clone(&side_effects.log);

        let error = enter_crossterm_terminal(&side_effects).unwrap_err();

        assert_eq!(error.to_string(), "a terminal session is already active");
        assert!(CROSSTERM_SESSION_ACTIVE.load(Ordering::SeqCst));
        assert!(log.lock().unwrap().is_empty());
    }

    #[test]
    fn mouse_capture_is_enabled_on_enter_and_disabled_on_leave() {
        let _guard = CROSSTERM_ACTIVE_TEST_LOCK.lock().unwrap();
        CROSSTERM_SESSION_ACTIVE.store(false, Ordering::SeqCst);
        let _reset = ResetCrosstermSessionActive;
        let side_effects = RecordingCrosstermSideEffects::default();
        let log = Arc::clone(&side_effects.log);

        enter_crossterm_terminal(&side_effects).unwrap();
        leave_crossterm_terminal(&side_effects).unwrap();

        assert_eq!(
            *log.lock().unwrap(),
            vec![
                "enable_raw_mode",
                "enter_alternate_screen",
                "enable_mouse_capture",
                "disable_mouse_capture",
                "leave_alternate_screen",
                "disable_raw_mode"
            ]
        );
    }
}
