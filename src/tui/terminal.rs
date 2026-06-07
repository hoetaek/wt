use anyhow::{Context, Result, bail};
use ratatui::crossterm::{
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
    fn leave(&self) -> Result<()>;
}

#[allow(dead_code)]
pub(crate) struct CrosstermEffects;

impl TerminalEffects for CrosstermEffects {
    fn enter(&self) -> Result<()> {
        enter_crossterm_terminal(&RealCrosstermSideEffects)
    }

    fn leave(&self) -> Result<()> {
        leave_crossterm_terminal(&RealCrosstermSideEffects)
    }
}

trait CrosstermSideEffects {
    fn enable_raw_mode(&self) -> Result<()>;
    fn enter_alternate_screen(&self) -> Result<()>;
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

    Ok(())
}

fn leave_crossterm_terminal(side_effects: &impl CrosstermSideEffects) -> Result<()> {
    if !CROSSTERM_SESSION_ACTIVE.load(Ordering::SeqCst) {
        return Ok(());
    }

    let screen_result = side_effects.leave_alternate_screen();
    let raw_mode_result = side_effects.disable_raw_mode();

    match (screen_result, raw_mode_result) {
        (Ok(()), Ok(())) => {
            CROSSTERM_SESSION_ACTIVE.store(false, Ordering::SeqCst);
            Ok(())
        }
        (Err(err), Ok(())) | (Ok(()), Err(err)) => Err(err),
        (Err(screen_err), Err(raw_mode_err)) => {
            Err(screen_err.context(format!("also failed to {raw_mode_err}")))
        }
    }
}

#[allow(dead_code)]
pub(crate) struct TerminalSession<E: TerminalEffects = CrosstermEffects> {
    effects: E,
    active: bool,
}

#[allow(dead_code)]
impl TerminalSession<CrosstermEffects> {
    pub(crate) fn new() -> Result<Self> {
        install_panic_hook_once();
        Self::with_effects(CrosstermEffects)
    }
}

#[allow(dead_code)]
impl<E: TerminalEffects> TerminalSession<E> {
    pub(crate) fn with_effects(effects: E) -> Result<Self> {
        effects.enter()?;
        Ok(Self {
            effects,
            active: true,
        })
    }

    pub(crate) fn suspend(&mut self) -> Result<()> {
        if self.active {
            self.effects.leave()?;
            self.active = false;
        }
        Ok(())
    }

    pub(crate) fn resume(&mut self) -> Result<()> {
        if !self.active {
            self.effects.enter()?;
            self.active = true;
        }
        Ok(())
    }
}

impl<E: TerminalEffects> Drop for TerminalSession<E> {
    fn drop(&mut self) {
        if self.active {
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
        assert_eq!(*log.lock().unwrap(), vec!["enter", "leave"]);
    }

    #[test]
    fn suspend_resume_pairs_leave_and_enter() {
        let effects = RecordingEffects::default();
        let log = Arc::clone(&effects.log);
        let mut session = TerminalSession::with_effects(effects).unwrap();
        session.suspend().unwrap();
        assert_eq!(*log.lock().unwrap(), vec!["enter", "leave"]);
        session.resume().unwrap();
        assert_eq!(*log.lock().unwrap(), vec!["enter", "leave", "enter"]);
        drop(session);
        assert_eq!(
            *log.lock().unwrap(),
            vec!["enter", "leave", "enter", "leave"]
        );
    }

    #[test]
    fn drop_after_suspend_does_not_double_leave() {
        let effects = RecordingEffects::default();
        let log = Arc::clone(&effects.log);
        let mut session = TerminalSession::with_effects(effects).unwrap();
        session.suspend().unwrap();
        drop(session);
        assert_eq!(*log.lock().unwrap(), vec!["enter", "leave"]);
    }

    #[test]
    fn suspend_twice_is_idempotent() {
        let effects = RecordingEffects::default();
        let log = Arc::clone(&effects.log);
        let mut session = TerminalSession::with_effects(effects).unwrap();
        session.suspend().unwrap();
        session.suspend().unwrap();
        assert_eq!(*log.lock().unwrap(), vec!["enter", "leave"]);
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
}
