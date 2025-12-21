# Jump code

- when I have a file with content: "abc" only, I invoke the jump action, enter a since I want to a, I see the hint `hh`. this is incorrect
two character hints should only appear when all single character hints are already present in the file.

- it should auto jump when there is only one hint displayed, like when file content is "abc" and user wants to jump to "a", so disregard the setting `auto_jump` code

- allow backspace to remove last typed in character

- when there are multi character hints displayed, and I already typed in the first character, the typed character hint should be in gray

---

- write in github about broken git blame in git commit view

- integrate latest changes from https://github.com/zed-industries/zed/pull/44770 (editor: Add smooth cursor animation)

- add argument to `workspace::NewFileFromClipboard` which allows to set initial language
then bind to space n j with json

- in `jump::Toggle` implement backspace to remove last typed in character

- check with main branch, fold anything via action, notice how in my fork, it ends up in visual mode
is that a bug from my fork code?

- improve UI `workspace::OpenRecentFile`. on very long file paths they are badly truncated
copy the design of the `file_finder::Toggle` action which shows file name left, then path at right truncated in gray

- in `workspace::NewFileFromClipboard` on initial opening the markdown block syntax highlighting is not working, at all
I always need to modify the content in the line before ```, then the syntax highlighting appears

related code:
`Self::new_in_workspace_with_content_and_language(workspace, content, Some("Markdown"), window, cx).detach_and_prompt_err(`

how about creating it with empty content and then afterward insert the clipboard content? try that out, maybe it will work

- document bugs with smooth caret about shifting character with video/screenshot, and disable in code that it does not animate in insert mode?

- do not use smooth caret in terminal, I think it causes lazygit commit message dialog typing weirdness (basically hidden)
even when smooth caret is disabled, in lazygit the cursor is weird?

- space u `tab_switcher::ToggleAll` should not show buffer where it was launched 

- implement swiper like search, or maybe via `tv`?
how about using project search like multibuffer, but only for current file with context=1?
I know that in the project panel with right click there is "Find in Folder...", can you create a new action which launches that functionality for the current file path prefilled like `dotty/.config/zed/keymap.json`. if the previous text buffer is in visual mode, preset the selection from visual mode into the "Search..." field
create a new action

- for `editor::SearchInCurrentFileViaMultiBuffer`, fix that when the previous buffer has a selection (like in vim mode), it does not take that as the initial text

# Misc

for key.l to jump to hints document bug that hint jump is not working in multi buffers (can it be fixed?)
maybe check latest fork code?

CHECK this:
- open README.md
- center the `New actions` headline content
- hit l to open the jump, then hit `b` to jump to `buffer`, notice how no jump hints appear (same with m for markdown)
is that because of uppercase character removal logic?
- hit l, then t works, but only displays characters at bottom?

---

Add actions to move to start and end of larger syntax node
https://github.com/zed-industries/zed/pull/45331

Add quick search modal
https://github.com/zed-industries/zed/pull/44530

## which key system

THIS is in MAIN already, check it out
https://github.com/zed-industries/zed/pull/43618

Add which-key system
https://github.com/zed-industries/zed/pull/34798

## side by side diffs

https://github.com/zed-industries/zed/issues/8279


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

Very small diff? Add smooth cursor animation
https://github.com/zed-industries/zed/pull/43826

## Add file explorer modal v2

https://github.com/zed-industries/zed/pull/45307

# Try out and maybe modify?

Beam Jump - Lightning Fast Vim style navigation
https://github.com/zed-industries/zed/pull/45387

# Potentially interesting things not merged

## Diff With Commit
https://github.com/zed-industries/zed/pull/44467

# AI (auggie) fails

- when I am inside a git commit view (for instance launched from git blame) and I run `git::Blame`, I just see this error notification: `failed to find a git repository for buffer`. I wonder if you can implement this, so `git::Blame` also works in git commit view tabs, and shows the left side next to the line numbers for the blame info PLUS `editor::OpenGitBlameCommit` works to jump to the new commit

- can you implement a new modal like `outline::Toggle`, there should be like a search text field input at top, focused, modal cancels on escape. you type in things and it showns all lines below (cap to 10 visible, candidate scrollable) and it is like a filter, so every line is shown below by default, but when I type "test", it only shows lines with test. on selecting one, jump to that instance, actually scrolling through candidates should live-update just like `outline::Toggle`. It is basically a line filter. Make sure that on long lines, you also consider horizontal scroll, so the searched text is always visible. Also highlight the matched text line part like `outline::Toggle`.
