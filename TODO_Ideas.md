- can `jump::Toggle` work in multi buffers? currently, no hints are displayed at all
I tried with AI and it fails to do, how about checking other jump hint PRs if it works there, and then copy over the relevant code?

- improve `jump::Toggle` to not allow entering extra characters, like currently one can enter " buffer" with leading space (plus buffer) and it trims down candidates. This is annoying because if you just want to jump to a space, only a few hints are shown, you first need to enter more. I want: Instantly after hitting space, I want to see all space characters highlighted to jump to (probably 2 characters in the ordering provided)

- improve `buffer_search_modal::ToggleBufferSearch` in `crates/search/src/buffer_search_modal.rs`
  - can the right preview side have soft wrap enabled? since otherwise, it is easy for the match to get out of view
  - can the left candidate be centered, currently is always at either top or bottom when holding arrow up/down (this is not implemented anywhere else in Zed, so probably too difficult to implement)
  - fix warnings once I am happy with it, then bind to key b and use zed regular at `space t b`

- how to enable AI predictions in those space t n buffers? Why does it have none?
see `fn edit_prediction_settings_at_position(`
the agent pane at right also has no edit predictions? Or does it?
try it out in `main`, then write bug report?

it interestingly worked with switching edit prediction provider to Codestral instead of `Zed AI`
is that a bug in the release version as well, try to reproduce

qwen suggested to use: `editor.update_edit_prediction_settings(cx);`, but it did not work
remove it from the code

- in visual line mode when cursor is on the newline character, then the line below is also incorrectly copied.
But when cursor is on the characters before on that line, it is correctly copied.
this also happens on `main`

- improve code around `MIN_NAVIGATION_HISTORY_ROW_DELTA` for proper jumping

---

# Investigations

## Smooth caret/cursor

### editor: Add smooth cursor animation (PR open)

I tested this and it has visual glitches, apparently which I documented in GitHub, so I do not use this.
It also does not support jumping the cursor across multiple panes.

https://github.com/zed-industries/zed/pull/44770

### Add smooth cursor animation (PR closed)

This has a very small diff, I checked out the branch, but `cargo run` does not start properly and is unable to open a window:

```
Zed failed to open a window: select toolchains

Caused by:
    0: Prepare call failed for query:
       SELECT
         name,
         path,
         worktree_id,
         relative_worktree_path,
         language_name,
         raw_json
       FROM
         toolchains
       WHERE
         workspace_id = ?
    1: Sqlite call failed with code 1 and message: Some("no such column: worktree_id"). See https://zed.dev/docs/linux for troubleshooting steps.
```

I then let AI apply the diff directly on my `dima` branch, and it correctly starts up and shows the smooth cursor.
But it has the same annoying character misplaced bug as the other diff, but in this PR it instantly jumps to the character of where the cursor will be which also looks bad.

https://github.com/zed-industries/zed/pull/43826

## Add file explorer modal v2 (PR open)

I already have his v1 (https://github.com/zed-industries/zed/pull/43961 (PR closed)) integrated. It is bound at `file_explorer::Toggle`.
I only see the v2 improvement that it has a full text field at the top, which can go outside the project root directory, but that is just a minor thing. I do not think I need it, since I can just do it via the `neovim` task.

https://github.com/zed-industries/zed/pull/45307

## telescope/quick search

Not so important with `buffer_search_modal::ToggleBufferSearch` and `editor::SearchInCurrentFileViaMultiBuffer`.

###  Add telescope style search (PR closed)

This was closed by Zed team in favor of the PR below.

I tested it, the file search only shows `...` which is not good. Text search seems very nice, otherwise, but the dialog is just too small designed for my resolution.

https://github.com/zed-industries/zed/pull/44942

### Add quick search modal (PR open)

I don't think it is ready yet, when a file has many search results, you do not see the file name anymore, it needs sticky scroll.
Otherwise, UI works great on my smaller resolution.

https://github.com/zed-industries/zed/pull/44530

## Filter for code actions

Absolutely not important since I rarely, if ever, need to search.

### Add filter for code actions (PR open)

Has merge conflicts and I do not have a clue how to merge.

https://github.com/zed-industries/zed/pull/44534

### Add fuzzy code actions picker (PR open)

This is a bit weird with a new action and numbers. Will not use it.

https://github.com/zed-industries/zed/pull/44802

## Git side by side diffs

Not so important.

### Basic side-by-side diff implementation (PR merged)

This is kinda difficult to enable, I stopped researching it.

https://github.com/zed-industries/zed/pull/43586

### Implement initial side-by-side Git diffs (PR closed)

PR was apparently closed, only has 3k changes.
Does it have merge conflicts?

https://github.com/zed-industries/zed/pull/40014

## Jump hint implementations

### The branch where my implementation is based on (no PR)

https://github.com/tebben/zed/tree/feature/jump

### Beam Jump - Lightning Fast Vim style navigation (PR open)

Has no screenshots.

https://github.com/zed-industries/zed/pull/45387

### helix: Add Helix's "Amp Jump" Feature (PR open)

This shows 2 character hints at the start of each word.

https://github.com/zed-industries/zed/pull/43733

# Impossible to fix from my side

## Fix that the git: blame action inside a git blame commit tab is not working and only showing an error notification

I tried to fix with my yek file merger through Gemini and via auggie, but both failed.

https://github.com/zed-industries/zed/issues/45532
