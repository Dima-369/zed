use collections::VecDeque;
use gpui::{Global, SharedString, UpdateGlobal};

const MAX_CLIPBOARD_HISTORY: usize = 100;

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
        if text.len() <= max_len {
            text.to_string().into()
        } else {
            let mut preview = text[..max_len].to_string();
            preview.push_str("…");
            preview.into()
        }
    }

    pub fn age_description(&self) -> String {
        if let Ok(duration) = self.timestamp.elapsed() {
            let secs = duration.as_secs();
            if secs < 60 {
                format!("{}s ago", secs)
            } else if secs < 3600 {
                format!("{}m ago", secs / 60)
            } else if secs < 86400 {
                format!("{}h ago", secs / 3600)
            } else {
                format!("{}d ago", secs / 86400)
            }
        } else {
            "just now".to_string()
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
        println!("[ClipboardHistory] add_entry called with text length: {}", text.len());
        // Don't add empty entries or duplicates of the most recent entry
        if text.is_empty() {
            println!("[ClipboardHistory] Skipping empty entry");
            return;
        }
        if let Some(last) = self.entries.front() {
            if last.text == text {
                println!("[ClipboardHistory] Skipping duplicate entry");
                return;
            }
        }

        println!("[ClipboardHistory] Adding new entry to history");
        self.entries.push_front(ClipboardEntry::new(text));
        if self.entries.len() > MAX_CLIPBOARD_HISTORY {
            self.entries.pop_back();
        }
        println!("[ClipboardHistory] Total entries now: {}", self.entries.len());
    }

    pub fn entries(&self) -> &VecDeque<ClipboardEntry> {
        &self.entries
    }
}

/// Helper function to track clipboard text in history
pub fn track_clipboard(text: &str, cx: &mut impl gpui::BorrowAppContext) {
    println!("[ClipboardHistory] track_clipboard called with: {:?}", &text[..text.len().min(50)]);
    ClipboardHistory::update_global(cx, |history, _| {
        println!("[ClipboardHistory] Before add_entry, history has {} entries", history.entries.len());
        history.add_entry(text.to_string());
        println!("[ClipboardHistory] After add_entry, history has {} entries", history.entries.len());
    });
}

pub fn init(cx: &mut gpui::App) {
    println!("[ClipboardHistory] Initializing clipboard_history crate");
    cx.set_global(ClipboardHistory::new());
}

