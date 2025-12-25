- can one tune the color of headlines in multi buffers?

# improve `buffer_search_modal::ToggleBufferSearch`

- Can the top candidate list be centered, currently is always at either top or bottom when holding arrow up/down?

I mean the selected row should be centered.
you can maybe check how the `crates/outline/src/outline.rs` or `crates/project_panel/src/project_panel.rs` does it. they don't have a full centered mode, but center a bit when using arrow up/down at edges


This is not implemented anywhere else in Zed, so probably too difficult to implement.

- fix that in no line mode the candidate item list lines have incorrect bottom padding, they look weird, the ones for the line mode are fine
weirdly, when no character is typed in, then in no line mode, the candidate rows have correct paddinge only as soon as anything is typed in.
It is hard to debug.

Reflect on 5-7 different possible sources of the problem, distill those down to 1-2 most likely sources, and then add logs to validate your assumptions before we move onto implementing the actual code fix.



# >>> Investigations

## Fix that edit predictions do not work for buffers without files, like ones started from workspace: new file

I fixed this in my own fork already, but let's see what Zed team says:

I created this bug report issue:
https://github.com/zed-industries/zed/issues/45631

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

v2 does not have the ignore files button/functionality anymore which sucks.

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



# >>> Impossible to fix from my side

## Fix that the git: blame action inside a git blame commit tab is not working and only showing an error notification

I tried to fix with my yek file merger through Gemini and via auggie, but both failed.

I created an issue for this:
https://github.com/zed-industries/zed/issues/45532

## improve `buffer_search_modal::ToggleBufferSearch` in `crates/search/src/buffer_search_modal.rs`

### Can the top candidate list be centered, currently is always at either top or bottom when holding arrow up/down?

I mean the selected row should be centered.

This is not implemented anywhere else in Zed, so probably too difficult to implement.

## improve `vim::HelixJumpToWord`

- in  `fn helix_handle_jump_input` can you make escape cancel out of the jump mode?

I tried, but escape is not propagated to `input_ignored`, so no idea how to fix this.
