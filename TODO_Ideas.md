- fix missing borders for tabs
always show borders for every tab, since you do not know which one is wrapped
where is code for pinned tabs?

- in visual line mode when cursor is on newline, then the line below is also incorrectly copied
is that a bug from my fork code?

- check with main branch, fold anything via action, notice how in my fork, it ends up in visual mode
is that a bug from my fork code?

# Try out

Add actions to move to start and end of larger syntax node
https://github.com/zed-industries/zed/pull/45331

## which key system

THIS is in MAIN already, check it out
https://github.com/zed-industries/zed/pull/43618

Add which-key system
https://github.com/zed-industries/zed/pull/34798

## telescope/quick search

Add telescope style search (this was closed by Zed team in favor of the PR below)
This looks nicer than the PR below, has more features?
https://github.com/zed-industries/zed/pull/44942

Add quick search modal
https://github.com/zed-industries/zed/pull/44530

## side by side diffs

figure out the way to enable this
Basic side-by-side diff implementation
https://github.com/zed-industries/zed/pull/43586

otherwise try this out
Implement initial side-by-side Git diffs
https://github.com/zed-industries/zed/pull/40014

## Filter for code actions

This seems nicer:
Add filter for code actions
https://github.com/zed-industries/zed/pull/44534

This is a bit weird with a new action and numbers:
Add fuzzy code actions picker
https://github.com/zed-industries/zed/pull/44802

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

## Add file explorer modal v2

I already have his v1 integrated, I think? Bound at `file_explorer::Toggle`

https://github.com/zed-industries/zed/pull/45307

# Potentially interesting things not merged

## Diff With Commit
https://github.com/zed-industries/zed/pull/44467

# AI (auggie) fails

- write in github about broken git blame in git commit view

- when I am inside a git commit view (for instance launched from git blame) and I run `git::Blame`, I just see this error notification: `failed to find a git repository for buffer`. I wonder if you can implement this, so `git::Blame` also works in git commit view tabs, and shows the left side next to the line numbers for the blame info PLUS `editor::OpenGitBlameCommit` works to jump to the new commit

## reconsider this, maybe by using excerpt lines = 1 or so?

- can you implement a new modal like `outline::Toggle`, there should be like a search text field input at top, focused, modal cancels on escape. you type in things and it showns all lines below (cap to 10 visible, candidate scrollable) and it is like a filter, so every line is shown below by default, but when I type "test", it only shows lines with test. on selecting one, jump to that instance, actually scrolling through candidates should live-update just like `outline::Toggle`. It is basically a line filter. Make sure that on long lines, you also consider horizontal scroll, so the searched text is always visible. Also highlight the matched text line part like `outline::Toggle`.
