# My workflow for macOS

To test modifications, I am only using `cargo run --no-default-features` (so it compiles without webrtc-sys) to compile and start Zed in debug mode which is faster than building the release binaries.

Once, I am satisfied with a batch of changes, I install Zed into `/Applications/Zed Dev.app` with this:

```bash
./script/bundle-mac-without-licenses -l -o -i && \
rm -f "$HOME/.cargo/bin/zed" && \
ln -s "/Applications/Zed Dev.app/Contents/MacOS/cli" "$HOME/.cargo/bin/zed"
```

## Sync this fork's main branch with Zed's main branch and merge into my custom dima branch

```bash
git checkout main && git pull zed main && git push && git checkout dima && git merge main
```

If there are merge conflicts, I resolve them via IntelliJ IDEA.

## Compare my changes with Zed's main branch

https://github.com/zed-industries/zed/compare/main...Dima-369:zed:dima

# Fork changes

## General/editor changes

- add many defaults in `project_settings.rs` to not crash on startup (not sure if that is only from my code)
- add `bundle-mac-without-licenses` which is faster than generating licenses, and skips the `sentry-cli` at end
- try to fix panic in `anchor_at_offset` when buffer has Umlaute, seems to work, no idea if my fix has other consequences
- changed `fn do_copy(&self, strip_leading_indents: bool, cx: &mut Context<Self>) {` to only strip trailing newlines instead of leading indents
- lower `MIN_NAVIGATION_HISTORY_ROW_DELTA` to 3, from 10, as a test which seems fine
- allow AI edit predictions in Zed's `settings.json` and `keymap.json` and in buffers without files like ones from `workspace: new file` or in the agent text thread pane (although here in the text thread it does not trigger as often?)
- opening a workspace which has no tabs initially, will trigger `workspace::NewFile` for proper editor focus. Before, there seems to be a bug where the project panel does not have proper focus
- improved the `go to next/previous diagnostic` action to always jump to errors first. Only if there are no errors, it jumps to warnings. Before, this was mixed
- moving up/down in outline panel does not wrap around anymore
- changed `agent::OpenActiveThreadAsMarkdown` to always open to end of buffer instead of start, and when there are more than 20k lines, open as `Plain Text` because Markdown lags hard for me, see `crates/agent_ui/src/acp/thread_view.rs` (the code for opening as plain text is still untested since I do not use agents inside Zed anymore, and just use CLI)
- add `vim_visual` context which can be set to `normal`, `line` or `block` for more fine-grained keybindings
- modified `vim/.../delete_motion.rs` so `vim::DeleteRight` at end of line stays on the newline character
- modified `editor::GoToDefinition` to not enter Vim visual mode when jumping to a definition
- fixed that a large `vertical_scroll_margin` in `settings.json` to have a centered cursor jumps buffer scrolls around (https://github.com/zed-industries/zed/issues/42155)
- fixed that on entering the project search, there can be instances where visual mode is entered (https://github.com/zed-industries/zed/issues/43878)
- integrated file explorer from https://github.com/zed-industries/zed/pull/43961
- integrated Helix jump list from https://github.com/zed-industries/zed/pull/44661 and implemented `vim::HelixOpenJumpListInMultibuffer` action
- add `blame > git_blame_font_family` setting to specify the font family for the git blame view because I am using a proportional font and the blame view misaligns otherwise
- integrated live refreshing project search from https://github.com/zed-industries/zed/pull/42889, enable in `settings.json` via `search > search_on_input`
- integrated smooth scroll from https://github.com/zed-industries/zed/pull/31671
- modified `compute_style_internal()` in `crates/gpui/src/elements/div.rs` to not apply the mouse hover style, since it clashes when one only uses the keyboard
  - I also unset the mouse hover background change on enabled `sticky_scroll`
- improved `outline::Toggle` to work in multi buffers, it shows the file headings only
- remove abbreviated `cwd` display in terminal title
- fix bug that when in vim visual line mode and cursor is on right newline character, that the line below is incorrectly copied on `editor::Copy`. This mostly happens in my own Zed config because I mixing `editor` and `vim` actions to ensure that I can move cursor on the right newline character, and usually not in proper Zed keybindings.
- improve `editor::SelectLargerSyntaxNode` for inline code blocks in Markdown files (`foo bar`), so that it first extends the selection to the word inside the quotes, then the text inside the quotes and only then to the inner text plus the outer quotes
- add structured outline for Markdown, modifies `crates/languages/src/markdown/outline.scm` (from https://github.com/zed-industries/zed/pull/45643)
- add a button to copy diagnostic messages from the hover popover to the clipboard (from https://github.com/zed-industries/zed/pull/45625)
- improve `file_finder::Toggle` matching to use substring through `nucleo` crate. I dislike fuzzy matching which is annoying. Based on https://github.com/zed-industries/zed/pull/37123, but that had fuzzy matching
- integrated 'Multibuffer breadcrumbs toolbar redesign' from https://github.com/zed-industries/zed/pull/45547

### Command palette

- the command palette sorting now sorts the same for `close work` and `work close`, and it does not search individual character matches anymore, like when you enter `bsp`, it would show `editor: backspace` before. I do not like that behavior, so I removed that
- changed `command palette: toggle` to sort by recency instead of hit count
- removed `GlobalCommandPaletteInterceptor` usage which contains Vim things like `:delete, :edit, :help, :join, :quit, :sort, :write, :xit, :yank` because I do not use them. Apparently, this also removed the ability to jump to a line via `:144`. I still removed this behavior because it is hard to sort those dynamic actions by recency in combination with the other real editor action commands.

## New actions

`workspace::OpenRecentFile` for recent file functionality which tracks every opened buffer to quickly jump to a recent file or open a recent workspace
- `Markdown::ScrollPageLittleDown` and `Markdown::ScrollPageLittleUp` which scroll a quarter of a page
- `projects::OpenRecentZoxide` which displays recent directories from `zoxide` CLI binary. It displays no footer and abbreviates paths to `~`. `highlighted_label.rs` was adjusted for its filtering. Here `cmd+enter` is flipped, so by default, it always opens in a new window
- `workspace::NewFileFromClipboard` which pastes in the clipboard contents
  - the action supports setting an initial language like `"space n j": [ "workspace::NewFileFromClipboard", { "language": "json" } ],` in `keymap.json`
- `workspace::CopyFilePaths` which opens a picker to copy the file path to clipboard
- `workspace::MakeSinglePane` which closes all other panes except the active one
- `snippets::ReloadSnippets` because auto-reloading snippets is not working for me
- `editor::CreateNavHistoryEntry`
- `editor::CopyAll` to copy entire buffer content to clipboard
- `editor::CountTokens` which counts the tokens in the current buffer using `o200k_base` via the `tiktoken` crate
- `editor::StopAllLanguageServers` which stops all language servers. It works like the bottom button in `Language Servers > Stop All Servers`
- `git::DiffWithCommit` from https://github.com/zed-industries/zed/pull/44467 and based on that code, `git::DiffWithBranch` is implemented
- `jump::Toggle` from https://github.com/tebben/zed/tree/feature/jump with the following changes:
  - modified key jump hints to my custom Dvorak Programmer keyboard layout
  - implemented multiple character jump hints
  - fixed bug that hints did not appear correctly
  - set the opacity of the dialog to 50% to see hints below
  - implemented `jump::JumpToUrl` based on this code to jump to `http...` URLs
  - note that it does not work in multi buffers, but it works to jump across panes of regular text editors
- `vim::HelixJumpToWord` from https://github.com/zed-industries/zed/pull/43733
  - improved UI to look like the `jump::Toggle` action
  - removed the `helix > "jump_label_accent"` setting since the UI is now the same as `jump::Toggle`
  - modified key jump hints to my custom Dvorak Programmer keyboard layout
  - I am only using this is inside multi buffers, whereas `jump::Toggle` does not. And this also does not work to jump across editor panes
  - note that escape does not work to break out of this mode, apparently. I have no idea how to adjust the code for it
- [DEPRECATED due to smooth scrolling PR merge] `editor::MoveLinesSmooth` which can be used like this. Do not set a too high `line_count` as it will keep scrolling even when key is released. It is not perfect, and sometimes, under high system load, it can happen that when you jump to top/bottom of file, it still scrolls a bit. Bind like this:

```json
"v": [
  "editor::MoveLinesSmooth",
  {
    "up": true,
    "line_count": 9,
    "delay_ms": 1
  }
],
```

-  `zed::DeeplTranslate` which translates the current selection or the current line. It needs the `DEEPL_API_KEY` environment variable to be set. Bind like this:

```json
"space c g": [
  "zed::DeeplTranslate",
  {
    "source_lang": "EN",
    "target_lang": "DE",
  }
],
```

- `editor::MoveToStartOfLargerSyntaxNode` from https://github.com/zed-industries/zed/pull/45331
- `buffer_search_modal::ToggleBufferSearch` which shows a modal to search the current buffer content (code is in `crates/search/src/buffer_search_modal.rs`) based on https://github.com/zed-industries/zed/pull/44530 (Add quick search modal). This is a basic implementation of Swiper from Emacs or `Snacks.picker.lines()` from Neovim. I tried matching every line with `nucleo`, but it was kinda slow, so it just split on spaces and then every line which has all words from the query is matched.
  - `ctrl-c` and `ctrl-t` can be used to insert history items into the search field
  - `ctrl-r` is to toggle between line (case-insensitive) and exact match (case-sensitive) mode
  - it also works in multi buffers, although the preview editor mixes lines

## UI changes

- on macOS, the unsaved changes model uses the native macOS dialog instead of Zed's custom one which has bad keyboard support, so `unsaved_changes_model.rs` was created which allows keyboard navigation (and just looks nicer)
- use larger font size (`LabelSize::Default`) for the line/column and selection info in the bottom bar and use `text_accent` for it when a selection is active
- lower excessive tab height
- lower status bar height, see `impl Render for StatusBar`
- add scrollbar to `outline::Toggle`, `file_finder::Toggle` and `command_palette::Toggle` (why is it not shown in the first place?)
- implement vertical tabs which go to next rows without scrollbars. Enable in `settings.json` with:

```json
"tab_bar": {
  "vertical_stacking": true
}
```

It places pinned tabs in an own row, separated to non-pinned tabs.
Since it was too difficult to only render tab borders where exactly required, every tab now has a full border, so it looks a bit bold between dividers, but I don't mind. It looks better that way, instead of missing top borders in second row, for instance, when first row has pinned tabs.

- lower `toolbar.rs` height to save space, same in `breadcrumbs.rs` (here no padding is set). This applies for terminals, as well
- switch system tab background color from `title_bar_background` to `tab_bar_background`, so I can style active tabs far nicer because the default just uses a slightly different foreground color which is hard to spot
- lower `DEFAULT_TOAST_DURATION` from 10 to 5 seconds
- lower horizontal scroll bar height to half of vertical one (the default one is huge)
- hide horizontal scroll bar when soft wrap is enabled

# Original README

# Zed

[![Zed](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/zed-industries/zed/main/assets/badge/v0.json)](https://zed.dev)
[![CI](https://github.com/zed-industries/zed/actions/workflows/run_tests.yml/badge.svg)](https://github.com/zed-industries/zed/actions/workflows/run_tests.yml)

Welcome to Zed, a high-performance, multiplayer code editor from the creators of [Atom](https://github.com/atom/atom) and [Tree-sitter](https://github.com/tree-sitter/tree-sitter).

---

### Installation

On macOS, Linux, and Windows you can [download Zed directly](https://zed.dev/download) or install Zed via your local package manager ([macOS](https://zed.dev/docs/installation#macos)/[Linux](https://zed.dev/docs/linux#installing-via-a-package-manager)/[Windows](https://zed.dev/docs/windows#package-managers)).

Other platforms are not yet available:

- Web ([tracking issue](https://github.com/zed-industries/zed/issues/5396))

### Developing Zed

- [Building Zed for macOS](./docs/src/development/macos.md)
- [Building Zed for Linux](./docs/src/development/linux.md)
- [Building Zed for Windows](./docs/src/development/windows.md)

### Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for ways you can contribute to Zed.

Also... we're hiring! Check out our [jobs](https://zed.dev/jobs) page for open roles.

### Licensing

License information for third party dependencies must be correctly provided for CI to pass.

We use [`cargo-about`](https://github.com/EmbarkStudios/cargo-about) to automatically comply with open source licenses. If CI is failing, check the following:

- Is it showing a `no license specified` error for a crate you've created? If so, add `publish = false` under `[package]` in your crate's Cargo.toml.
- Is the error `failed to satisfy license requirements` for a dependency? If so, first determine what license the project has and whether this system is sufficient to comply with this license's requirements. If you're unsure, ask a lawyer. Once you've verified that this system is acceptable add the license's SPDX identifier to the `accepted` array in `script/licenses/zed-licenses.toml`.
- Is `cargo-about` unable to find the license for a dependency? If so, add a clarification field at the end of `script/licenses/zed-licenses.toml`, as specified in the [cargo-about book](https://embarkstudios.github.io/cargo-about/cli/generate/config.html#crate-configuration).
