- implement swiper like search, or maybe via `tv`?
how about using project search like multibuffer, but only for current file with context=1?
I know that in the project panel with right click there is "Find in Folder...", can you create a new action which launches that functionality for the current file path prefilled like `dotty/.config/zed/keymap.json`. if the previous text buffer is in visual mode, preset the selection from visual mode into the "Search..." field
create a new action

can you implement a new modal like `outline::Toggle`, there should be like a search text field input at top, focused, modal cancels on escape. you type in things and it showns all lines below (cap to 10 visible, candidate scrollable) and it is like a filter, so every line is shown below by default, but when I type "test", it only shows lines with test. on selecting one, jump to that instance, actually scolling through candidates should live-update just like `outline::Toggle`. It is basically a line filter. Make sure that on long lines, you also consider horizontal scroll, so the searched text is always visible. Also highlight the matched text line part.

- for `editor::SearchInCurrentFileViaMultiBuffer`, fix that when the previous buffer has a selection (like in vim mode), it does not take that as the initial text

# AI fails

(auggie failed)
when I am inside a git commit view (for instance launched from git blame) and I run `git::Blame`, I just see this error notification: `failed to find a git repository for buffer`. I wonder if you can implement this, so `git::Blame` also works in git commit view tabs, and shows the left side next to the line numbers for the blame info PLUS `editor::OpenGitBlameCommit` works to jump to the new commit
