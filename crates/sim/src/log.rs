use hearth_ecs::Entity;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogKind {
    Info,
    Funny,
    Social,
    Career,
    Life,
    Death,
    Money,
    Warning,
    Debug,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub tick: u64,
    pub kind: LogKind,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subject: Option<Entity>,
}

/// Narrative event log. Drives UI notifications and lets tests assert on what happened.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct SimLog {
    pub entries: Vec<LogEntry>,
    /// Entries not yet shown as notifications.
    pub unread: usize,
    /// Count of all entries ever written (entries may be trimmed).
    pub total: u64,
    #[serde(skip)]
    pub echo: bool,
}

impl SimLog {
    pub const CAP: usize = 4000;

    pub fn push(&mut self, tick: u64, kind: LogKind, subject: Option<Entity>, text: impl Into<String>) {
        let text = text.into();
        if self.echo {
            println!("[{:>6}] {:?}: {}", tick, kind, text);
        }
        self.entries.push(LogEntry { tick, kind, text, subject });
        self.unread += 1;
        self.total += 1;
        if self.entries.len() > Self::CAP {
            let drop = self.entries.len() - Self::CAP;
            self.entries.drain(..drop);
        }
    }

    pub fn count(&self, kind: LogKind) -> usize {
        self.entries.iter().filter(|e| e.kind == kind).count()
    }

    pub fn contains(&self, needle: &str) -> bool {
        self.entries.iter().any(|e| e.text.contains(needle))
    }
}
