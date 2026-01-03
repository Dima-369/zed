use collections::VecDeque;
use gpui::{Global, SharedString, UpdateGlobal};

const MAX_CLIPBOARD_HISTORY: usize = 300;

#[derive(Clone, Debug)]
pub struct ClipboardEntry {
    pub text: String,
    pub timestamp: std::time::SystemTime,
}

impl ClipboardEntry {
    pub fn new(text: String) -> Self {
        Self {
            text,
            timestamp: std::time::SystemTime::now(),
        }
    }

    pub fn preview(&self) -> SharedString {
        let text = self.text.trim();
        let max_len = 500;

        // Replace newlines with ⏎ symbol
        let text_with_newline_symbols = text.replace('\n', "⏎");

        if text_with_newline_symbols.len() <= max_len {
            text_with_newline_symbols.into()
        } else {
            let mut preview = text_with_newline_symbols[..max_len].to_string();
            preview.push_str("…");
            preview.into()
        }
    }
}

pub struct ClipboardHistory {
    entries: VecDeque<ClipboardEntry>,
}

impl Default for ClipboardHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl Global for ClipboardHistory {}

impl ClipboardHistory {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_CLIPBOARD_HISTORY),
        }
    }

    pub fn add_entry(&mut self, text: String) {
        // Don't add empty entries, very short entries (<=3 chars), or duplicates of the most recent entry
        if text.is_empty() || text.len() <= 3 {
            return;
        }
        if let Some(last) = self.entries.front() {
            if last.text == text {
                return;
            }
        }

        self.entries.push_front(ClipboardEntry::new(text));
        if self.entries.len() > MAX_CLIPBOARD_HISTORY {
            self.entries.pop_back();
        }
    }

    pub fn entries(&self) -> &VecDeque<ClipboardEntry> {
        &self.entries
    }
}

/// Helper function to track clipboard text in history
pub fn track_clipboard(text: &str, cx: &mut impl gpui::BorrowAppContext) {
    ClipboardHistory::update_global(cx, |history, _| {
        history.add_entry(text.to_string());
    });
}

pub fn init(cx: &mut gpui::App) {
    cx.set_global(ClipboardHistory::new());
}

