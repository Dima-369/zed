use gpui::{Entity, SharedString};
use language::{Anchor, Buffer};
use project::Symbol;

pub struct SearchResult {
    pub label: SharedString,
    pub detail: Option<SharedString>,
    pub symbol: Option<Symbol>,
    pub document_symbol: Option<DocumentSymbolResult>,
}

#[derive(Clone)]
pub struct DocumentSymbolResult {
    pub buffer: Entity<Buffer>,
    pub range: std::ops::Range<Anchor>,
}
