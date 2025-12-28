- remove `editor::SearchInCurrentFileViaMultiBuffer`

- unify text thread and agent editor keybindings around enter and cmd-enter, also think about vim_mode insert and normal

# ACP

- can ACP threads actually preview command output? maybe just last 20 lines with little success/error indication. Running it just states that it runs something but shows nothing which sucks, Windsurf embeds a real small editor where one can even input sudo passwords, but I really don't need it to be that fancy

- add 2 new actions to `agent::...`. First DismissErrorNotification and second CopyErrorNotification

- 

- can ACP have a whitelist/blacklist in settings of CLI commands to be able to be run? I think an array of regexes in settings would be great

# Support external agent history

https://github.com/zed-industries/zed/pull/45734

# agent: History and recent conversations persistence per workspace 

https://github.com/zed-industries/zed/pull/41874


# adjust zed cli, add a new flag for when opened via `zed -`, that it should position cursor at end

- test out terminal integration via `zed - --stdin-cursor-at-end` for terminal scrollback buffer once Zed Dev is compiled




# >>> Investigations

## edit predictions: Amp Tab support

BUT I tested while the PR was not done!
The completions suck hard! Often wants to jump to something off-screen in file and does weird edits.
https://github.com/zed-industries/zed/pull/45724

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

I tried out the branch and I really don't think I need it. I think it also mixes sorting of files and directories?

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

### Center top candidates list always

Can the top candidate list be centered, currently is always at either top or bottom when holding arrow up/down? I mean the selected row should be centered. Real centered movement is not implemented anywhere else in Zed, so too difficult to implement. I tried with Windsurf Penguin Alpha and it was not able to.

This is not implemented anywhere else in Zed, so probably too difficult to implement.

### Incorrect bottom padding in no line mode

Fix that in no line mode the candidate item list lines have incorrect bottom padding, They look weird, the ones for the line mode are fine weirdly, when no character is typed in, then in no line mode, the candidate rows have correct paddinge only as soon as anything is typed in.

I have no idea why this is happening, and it would be amazing to fix, but I can not figure it out.
