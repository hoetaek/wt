use crate::context::UserInterface;
use crate::error::WtError;
use anyhow::{Result, bail};
use std::sync::{Mutex, mpsc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrintKind {
    Step,
    Plain,
    Dim,
    Warning,
    Error,
}

pub(crate) enum UiRequest {
    Confirm {
        prompt: String,
        default: bool,
        reply: mpsc::Sender<UiReply>,
    },
    Select {
        prompt: String,
        items: Vec<String>,
        reply: mpsc::Sender<UiReply>,
    },
    MultiSelect {
        prompt: String,
        items: Vec<String>,
        reply: mpsc::Sender<UiReply>,
    },
    Input {
        prompt: String,
        default: Option<String>,
        reply: mpsc::Sender<UiReply>,
    },
    Print {
        kind: PrintKind,
        line: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UiReply {
    Bool(bool),
    Index(usize),
    Indices(Vec<usize>),
    Text(String),
    Cancelled,
}

pub(crate) struct TuiUi {
    tx: Mutex<mpsc::Sender<UiRequest>>,
}

impl TuiUi {
    pub(crate) fn new(tx: mpsc::Sender<UiRequest>) -> Self {
        Self { tx: Mutex::new(tx) }
    }

    fn send_request(&self, request: UiRequest) -> Result<()> {
        self.tx
            .lock()
            .map_err(|_| cancelled_error())?
            .send(request)
            .map_err(|_| cancelled_error())
    }

    fn send_print(&self, kind: PrintKind, line: &str) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(UiRequest::Print {
                kind,
                line: line.to_string(),
            });
        }
    }
}

impl UserInterface for TuiUi {
    fn select(&self, prompt: &str, items: &[String]) -> Result<usize> {
        let (reply, replies) = mpsc::channel();
        self.send_request(UiRequest::Select {
            prompt: prompt.to_string(),
            items: items.to_vec(),
            reply,
        })?;

        match replies.recv().map_err(|_| cancelled_error())? {
            UiReply::Index(index) => Ok(index),
            UiReply::Cancelled => Err(cancelled_error()),
            reply => bail!("prompt '{prompt}' returned unexpected reply: {reply:?}"),
        }
    }

    fn multi_select(&self, prompt: &str, items: &[String]) -> Result<Vec<usize>> {
        let (reply, replies) = mpsc::channel();
        self.send_request(UiRequest::MultiSelect {
            prompt: prompt.to_string(),
            items: items.to_vec(),
            reply,
        })?;

        match replies.recv().map_err(|_| cancelled_error())? {
            UiReply::Indices(indices) => Ok(indices),
            UiReply::Cancelled => Err(cancelled_error()),
            reply => bail!("prompt '{prompt}' returned unexpected reply: {reply:?}"),
        }
    }

    fn confirm(&self, prompt: &str, default: bool) -> Result<bool> {
        let (reply, replies) = mpsc::channel();
        self.send_request(UiRequest::Confirm {
            prompt: prompt.to_string(),
            default,
            reply,
        })?;

        match replies.recv().map_err(|_| cancelled_error())? {
            UiReply::Bool(value) => Ok(value),
            UiReply::Cancelled => Err(cancelled_error()),
            reply => bail!("prompt '{prompt}' returned unexpected reply: {reply:?}"),
        }
    }

    fn input(&self, prompt: &str, default: Option<&str>) -> Result<String> {
        let (reply, replies) = mpsc::channel();
        self.send_request(UiRequest::Input {
            prompt: prompt.to_string(),
            default: default.map(str::to_string),
            reply,
        })?;

        match replies.recv().map_err(|_| cancelled_error())? {
            UiReply::Text(text) => Ok(text),
            UiReply::Cancelled => Err(cancelled_error()),
            reply => bail!("prompt '{prompt}' returned unexpected reply: {reply:?}"),
        }
    }

    fn print_step(&self, msg: &str) {
        self.send_print(PrintKind::Step, msg);
    }

    fn print_plain(&self, msg: &str) {
        self.send_print(PrintKind::Plain, msg);
    }

    fn print_dim(&self, msg: &str) {
        self.send_print(PrintKind::Dim, msg);
    }

    fn print_warning(&self, msg: &str) {
        self.send_print(PrintKind::Warning, msg);
    }

    fn print_error(&self, msg: &str) {
        self.send_print(PrintKind::Error, msg);
    }
}

fn cancelled_error() -> anyhow::Error {
    anyhow::Error::new(WtError::Cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::UserInterface;
    use crate::error::WtError;
    use std::sync::mpsc;

    #[test]
    fn confirm_round_trips_reply_and_blocks_until_answer() {
        let (tx, rx) = mpsc::channel();
        let ui = TuiUi::new(tx);
        let worker =
            std::thread::spawn(move || ui.confirm("Pull selected provider fields?", false));
        let UiRequest::Confirm {
            prompt,
            default,
            reply,
        } = rx.recv().unwrap()
        else {
            panic!("expected Confirm request");
        };
        assert_eq!(prompt, "Pull selected provider fields?");
        assert!(!default);
        reply.send(UiReply::Bool(true)).unwrap();
        assert!(worker.join().unwrap().unwrap());
    }

    #[test]
    fn cancelled_reply_maps_to_wt_error_cancelled() {
        let (tx, rx) = mpsc::channel();
        let ui = TuiUi::new(tx);
        let worker = std::thread::spawn(move || ui.input("Issue id to attach", None));
        let UiRequest::Input { reply, .. } = rx.recv().unwrap() else {
            panic!("expected Input request");
        };
        reply.send(UiReply::Cancelled).unwrap();
        let err = worker.join().unwrap().unwrap_err();
        assert!(matches!(
            err.downcast_ref::<WtError>(),
            Some(WtError::Cancelled)
        ));
    }

    #[test]
    fn dropped_ui_receiver_maps_to_cancelled() {
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let ui = TuiUi::new(tx);
        let err = ui.confirm("still there?", true).unwrap_err();
        assert!(matches!(
            err.downcast_ref::<WtError>(),
            Some(WtError::Cancelled)
        ));
    }

    #[test]
    fn select_round_trips_items_and_index() {
        let (tx, rx) = mpsc::channel();
        let ui = TuiUi::new(tx);
        let worker = std::thread::spawn(move || {
            ui.multi_select("Tasks to publish", &["a".to_string(), "b".to_string()])
        });
        let UiRequest::MultiSelect { items, reply, .. } = rx.recv().unwrap() else {
            panic!("expected MultiSelect request");
        };
        assert_eq!(items, vec!["a".to_string(), "b".to_string()]);
        reply.send(UiReply::Indices(vec![1])).unwrap();
        assert_eq!(worker.join().unwrap().unwrap(), vec![1]);
    }

    #[test]
    fn print_is_fire_and_forget_with_kind() {
        let (tx, rx) = mpsc::channel();
        let ui = TuiUi::new(tx);
        ui.print_warning("conflicts present");
        let UiRequest::Print { kind, line } = rx.try_recv().unwrap() else {
            panic!("expected Print request");
        };
        assert_eq!(kind, PrintKind::Warning);
        assert_eq!(line, "conflicts present");
    }

    #[test]
    fn plain_print_keeps_plain_kind() {
        let (tx, rx) = mpsc::channel();
        let ui = TuiUi::new(tx);
        ui.print_plain("Pull preview");
        let UiRequest::Print { kind, line } = rx.try_recv().unwrap() else {
            panic!("expected Print request");
        };
        assert_eq!(kind, PrintKind::Plain);
        assert_eq!(line, "Pull preview");
    }

    #[test]
    fn mismatched_reply_variant_is_an_error_not_a_panic() {
        let (tx, rx) = mpsc::channel();
        let ui = TuiUi::new(tx);
        let worker = std::thread::spawn(move || ui.confirm("ok?", false));
        let UiRequest::Confirm { reply, .. } = rx.recv().unwrap() else {
            panic!("expected Confirm request");
        };
        reply.send(UiReply::Text("oops".into())).unwrap();
        assert!(worker.join().unwrap().is_err());
    }
}
