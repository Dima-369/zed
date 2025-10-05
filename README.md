# My workflow for macOS

To test modifications, I am only using `cargo run` to compile and start Zed in debug mode which is faster than building the release binaries.

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

## Compare my changes with Zed's main branch

https://github.com/zed-industries/zed/compare/main...Dima-369:zed:dima

# Fork changes

## General/editor changes

- add `bundle-mac-without-licenses` which is faster than generating licenses, and skips the `sentry-cli` at end
- try to fix panic in `anchor_at_offset` when buffer has Umlaute, seems to work, no idea if my fix has other consequences
- changed `fn do_copy(&self, strip_leading_indents: bool, cx: &mut Context<Self>) {` to only strip trailing newlines instead of leading indents
- lower `MIN_NAVIGATION_HISTORY_ROW_DELTA` to 3, from 10, as a test which seems fine
- allow AI edit predictions in Zed's `settings.json` and `keymap.json`
- opening a workspace which has no tabs initially, will trigger `workspace::NewFile` for proper editor focus. Before, there seems to be a bug where the project panel does not have proper focus
- implement new recent file functionality which tracks every opened buffer to quickly jump to file/open new workspace. Action is `workspace::OpenRecentFile`
- added new setting: `diagnostics > merge_same_range` to merge diagnostics which have the same character range (I noticed this in Gleam's LSP: https://github.com/gleam-lang/gleam/issues/4946)
- go to next or previous diagnostic always first jumps to errors, and only if there are no errors, it jumps to warnings. Before, it was mixed
- moving up/down in outline panel does not wrap around anymore
- changed `agent::OpenActiveThreadAsMarkdown` to always open to end of buffer instead of start, and when there are more than 20k lines, open as `Plain Text` because Markdown lags hard for me, see `crates/agent_ui/src/acp/thread_view.rs` (the second part is still untested)
- add `vim_visual` context which can be set to `normal`, `line` or `block` for more fine-grained keybindings

### Command palette

- the command palette sorting now sorts the same for `close work` and `work close`, and it does not search individual character matches like when you enter `clo wo`, it does not show `search: toggle whole word` because every individual character is contained
- changed `command palette: toggle` to sort by recency instead of hit count

## New actions

- `Markdown::ScrollPageLittleDown` and `Markdown::ScrollPageLittleUp` which scroll a quarter of a page
- `projects::OpenRecentZoxide` which displays recent directories from `zoxide` CLI binary. It displays no footer and abbreviates paths to `~`. `highlighted_label.rs` was adjusted for its filtering. Here `cmd+enter` is flipped, so by default, it always opens in a new window
- `workspace::NewFileFromClipboard` which pastes in the clipboard contents and sets `Markdown` language
- `workspace::CopyFilePaths` which opens a picker to copy the file path to clipboard
- `snippets::ReloadSnippets` because auto-reloading snippets is not working for me
- `editor::CreateNavHistoryEntry`
- `editor::CopyAll` to copy entire buffer content to clipboard
- `editor::CountTokens` which counts the tokens in the current buffer using `o200k_base` via the `tiktoken` crate
- `editor::MoveLinesSmooth` which can be used like this. Do not set a too high `line_count` as it will keep scrolling even when key is released. It is not perfect, and sometimes, under high system load, it can happen that when you jump to top/bottom of file, it still scrolls a bit. Bind like this:

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

## UI changes

- use larger font size (`LabelSize::Default`) for the line/column and selection info in the bottom bar and use `text_accent` for it when a selection is active
- lower excessive tab height
- lower status bar height, see `impl Render for StatusBar`
- middle clicking a system tab will close it, just like regular tabs
- lower `toolbar.rs` height to make it as minimal as possible to save space, same in `breadcrumbs.rs`. This applies for terminals, as well
- switch system tab background color from `title_bar_background` to `tab_bar_background`, so I can style active tabs far nicer because the default just uses a slightly different foreground color which is hard to spot
- highlight the active search match with a different background color. It is not trivial to set the foreground color to a fixed color, so I stopped trying
- lower `DEFAULT_TOAST_DURATION` from 10 to 5 seconds
- lower horizontal scroll bar height to half of vertical one (the default one is huge)
- hide horizontal scroll bar when soft wrap is enabled
- move search bar hit count `1/30` below the search text field, above the replace text field because it is hard too see at very right

# Original README

# Zed

[![Zed](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/zed-industries/zed/main/assets/badge/v0.json)](https://zed.dev)
[![CI](https://github.com/zed-industries/zed/actions/workflows/ci.yml/badge.svg)](https://github.com/zed-industries/zed/actions/workflows/ci.yml)

Welcome to Zed, a high-performance, multiplayer code editor from the creators of [Atom](https://github.com/atom/atom) and [Tree-sitter](https://github.com/tree-sitter/tree-sitter).

---

### Installation

On macOS and Linux you can [download Zed directly](https://zed.dev/download) or [install Zed via your local package manager](https://zed.dev/docs/linux#installing-via-a-package-manager).

Other platforms are not yet available:

- Windows ([tracking issue](https://github.com/zed-industries/zed/issues/5394))
- Web ([tracking issue](https://github.com/zed-industries/zed/issues/5396))

### Developing Zed

- [Building Zed for macOS](./docs/src/development/macos.md)
- [Building Zed for Linux](./docs/src/development/linux.md)
- [Building Zed for Windows](./docs/src/development/windows.md)
- [Running Collaboration Locally](./docs/src/development/local-collaboration.md)

### Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for ways you can contribute to Zed.

Also... we're hiring! Check out our [jobs](https://zed.dev/jobs) page for open roles.

### Licensing

License information for third party dependencies must be correctly provided for CI to pass.

We use [`cargo-about`](https://github.com/EmbarkStudios/cargo-about) to automatically comply with open source licenses. If CI is failing, check the following:

- Is it showing a `no license specified` error for a crate you've created? If so, add `publish = false` under `[package]` in your crate's Cargo.toml.
- Is the error `failed to satisfy license requirements` for a dependency? If so, first determine what license the project has and whether this system is sufficient to comply with this license's requirements. If you're unsure, ask a lawyer. Once you've verified that this system is acceptable add the license's SPDX identifier to the `accepted` array in `script/licenses/zed-licenses.toml`.
- Is `cargo-about` unable to find the license for a dependency? If so, add a clarification field at the end of `script/licenses/zed-licenses.toml`, as specified in the [cargo-about book](https://embarkstudios.github.io/cargo-about/cli/generate/config.html#crate-configuration).
