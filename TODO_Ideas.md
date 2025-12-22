- how to disable smooth caret?

- undo all smooth caret changes
711068f0facd is the merge commit
then merge in https://github.com/zed-industries/zed/pull/43826 as a test 

- how to enable AI predictions in those space t n buffers? Why does it have none?

- in visual line mode when cursor is on newline, then the line below is also incorrectly copied
is that a bug from my fork code?

- based on the code in https://github.com/zed-industries/zed/pull/44530 implement swiper
use 50% width left for candidates, right 50% width for preview

# AI (auggie) fails

- write in github about broken git blame in git commit view

- when I am inside a git commit view (for instance launched from git blame) and I run `git::Blame`, I just see this error notification: `failed to find a git repository for buffer`. I wonder if you can implement this, so `git::Blame` also works in git commit view tabs, and shows the left side next to the line numbers for the blame info PLUS `editor::OpenGitBlameCommit` works to jump to the new commit




---

# Investigations

## Smooth cursor

### editor: Add smooth cursor animation

I tested this and it has visual glitches, apparently which I documented in GitHub, so I do not use this.
It also does not support jumping the cursor across multiple panes.

https://github.com/zed-industries/zed/pull/44770

### Add smooth cursor animation

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

https://github.com/zed-industries/zed/pull/43826

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

## Jump hint implementations

### The branch where my implementation is based on (no PR)

https://github.com/tebben/zed/tree/feature/jump

### Beam Jump - Lightning Fast Vim style navigation (PR open)

Has no screenshots.

https://github.com/zed-industries/zed/pull/45387

### helix: Add Helix's "Amp Jump" Feature (PR open)

This shows 2 character hints at the start of each word.

https://github.com/zed-industries/zed/pull/43733
