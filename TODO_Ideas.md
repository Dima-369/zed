- is there a keybinding to view the commit from git blame?
does not seem there is, when I am inside a git commit view and I run `git::Blame`, I just see this error notification: `failed to find a git repository for buffer`. I wonder if you can implement this, so `git::Blame` also works in git commit view tabs, and shows the left side next to the line numbers for the blame info PLUS `editor::OpenGitBlameCommit` works to jump to the new commit

- implement swiper like search, or maybe via `tv`?
how about using project search like multibuffer, but only for current file with context=1?
I know that in the project panel with right click there is "Find in Folder...", can you create a new action which launches that functionality for the current file path prefilled like `dotty/.config/zed/keymap.json`. if the previous text buffer is in visual mode, preset the selection from visual mode into the "Search..." field
create a new action

- for `editor::SearchInCurrentFileViaMultiBuffer`, fix that when the previous buffer has a selection (like in vim mode), it does not take that as the initial text

- try out:

editor: Implement inline references (peek references) #44669
https://github.com/zed-industries/zed/pull/44669

fix that in vim::HelixOpenJumpListInMultibuffer it launches a multi selection cursor, it should just be a single cursor, and open at first match
