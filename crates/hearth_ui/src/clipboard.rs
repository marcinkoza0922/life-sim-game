//! The system clipboard, supplied by the host (it owns the window and display connection).

/// Where text fields copy to and paste from. Set [`crate::Ui::clipboard`] to a system-backed
/// implementation; the default keeps text in memory, which is enough for tests and for copying
/// between fields of the same game.
pub trait Clipboard {
    fn get(&mut self) -> Option<String>;
    fn set(&mut self, text: String);
}

/// An in-process clipboard.
#[derive(Default, Debug)]
pub struct LocalClipboard(pub Option<String>);

impl Clipboard for LocalClipboard {
    fn get(&mut self) -> Option<String> {
        self.0.clone()
    }
    fn set(&mut self, text: String) {
        self.0 = Some(text);
    }
}
