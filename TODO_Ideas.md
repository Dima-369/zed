- how to enable AI predictions in those space t n buffers? Why does it have none?

- in visual line mode when cursor is on newline, then the line below is also incorrectly copied
is that a bug from my fork code?

## Smooth cursor

editor: Add smooth cursor animation
This is integrated, only `inertial_cursor.rs` is implemented, not the other VFX modes.
https://github.com/zed-industries/zed/pull/44770

- integrate latest changes from https://github.com/zed-industries/zed/pull/44770 (editor: Add smooth cursor animation)

- document bugs with smooth caret about shifting character with video/screenshot, and disable in code that it does not animate in insert mode?

- do not use smooth caret in terminal, I think it causes lazygit commit message dialog typing weirdness (basically hidden)
even when smooth caret is disabled, in lazygit the cursor is weird?

---

Very small diff? Add smooth cursor animation
https://github.com/zed-industries/zed/pull/43826

## Diff With Commit

Start with this, then implement diff with branch in modal as well.
https://github.com/zed-industries/zed/pull/44467

# AI (auggie) fails

- write in github about broken git blame in git commit view

- when I am inside a git commit view (for instance launched from git blame) and I run `git::Blame`, I just see this error notification: `failed to find a git repository for buffer`. I wonder if you can implement this, so `git::Blame` also works in git commit view tabs, and shows the left side next to the line numbers for the blame info PLUS `editor::OpenGitBlameCommit` works to jump to the new commit

## reconsider this, maybe by using excerpt lines = 1 or so?

- can you implement a new modal like `outline::Toggle`, there should be like a search text field input at top, focused, modal cancels on escape. you type in things and it showns all lines below (cap to 10 visible, candidate scrollable) and it is like a filter, so every line is shown below by default, but when I type "test", it only shows lines with test. on selecting one, jump to that instance, actually scrolling through candidates should live-update just like `outline::Toggle`. It is basically a line filter. Make sure that on long lines, you also consider horizontal scroll, so the searched text is always visible. Also highlight the matched text line part like `outline::Toggle`.

# Investigations

## Add file explorer modal v2 (PR open)

I already have his v1 (https://github.com/zed-industries/zed/pull/43961 (PR closed)) integrated. It is bound at `file_explorer::Toggle`.
I only see the v2 improvement that it has a full text field at the top, which can go outside the project root directory, but that is just a minor thing.

https://github.com/zed-industries/zed/pull/45307

## telescope/quick search

Not so important with `editor::SearchInCurrentFileViaMultiBuffer`.

###  Add telescope style search (PR closed)

This was closed by Zed team in favor of the PR below.

I tested it, the file search only shows `...` which is not good. Text search seems very nice, otherwise, but the dialog is just too small designed for my resolution.

https://github.com/zed-industries/zed/pull/44942

### Add quick search modal (PR WIP)

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
