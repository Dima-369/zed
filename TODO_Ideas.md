- try out AMP tab once again
https://github.com/zed-industries/zed/pull/45724

- can buffer search modal be used properly in AI text threads? it currently does not work

- in Zed Agent with Qwen, why do I not see tool usages, for reading files?
does it also happen in Release Zed on say a free OpenRouter model?
it seems to work for Raptor model?
MAYBE it is because the code is a bit incorrect, I see the modal duplicating parts of its response, see text thread
and also in the tab summary for text threads, first tokens are always duplicated

- start fixing all unit tests

- pull in latest https://github.com/zed-industries/zed/pull/45734 changes (agent history)

# Better Agent/text thread title summaries

`crates/agent_settings/src/prompts/summarize_thread_prompt.txt` is used for summary

maybe use the polling mechanism from here to detect when agent is idle to generate the tab summary for agent threads (external or not)
https://github.com/zed-industries/zed/pull/45930 (feat: Add Ctrl+Shift+Enter to queue messages until agent finishes)

- currently, ACP thread summaries are generated after the first message is received from AI which very often is not good enough? or is it really? can you investigate code path and check when it is generated, it is shown in the tab title

- the AI tab title summary is updated far too often in Zed Agent, on every AI message received, but it should only be updated when the AI is fully done responding, when its loop is done. It should also be done in text threads and in ACP threads (external agents)
So, 3 parts, check all
see `crates/agent_ui/src/agent_panel.rs` and related code

# git_ui: Implement interactive Git commit graph view

check this out

https://github.com/zed-industries/zed/pull/45884

# Improve Git Panel with TreeView, VSCode-style grouping, commit history, and auto-fetch

try this out some time, I already have tree view, how about only displaying file count in tree view to the right of directories only for amount of files below

https://github.com/zed-industries/zed/pull/45846

# ACP

- can ACP have a whitelist/blacklist in settings of CLI commands to be able to be run?
first investigate without changing code how it currently works and where it stores the info when I click "Allow always"
I think an array of regexes in settings would be great


## Current ACP Permission System

### How "Allow always" currently works:

1. **Permission Request Flow**: When an ACP agent wants to run a CLI command, it sends a permission request to Zed via the `agent-client-protocol`

2. **UI Response**: Zed shows permission options (Allow once, Allow always, Reject once, Reject always) in the UI

3. **Storage Location**: The "Allow always" information is **NOT stored in Zed's settings**. Instead, it's stored and managed by the **external agent server** (like Claude Code, Gemini CLI, etc.)

4. **Protocol Communication**: The permission decision is sent back to the external agent via the `RequestPermissionResponse` in the agent-client-protocol

5. **Agent-side Enforcement**: The external agent maintains its own whitelist/blacklist and decides whether to ask for permissions again based on previous "Allow always" responses

### Key Code Locations:

- **Permission UI**: [/Users/dima/Developer/zed/crates/agent_ui/src/acp/thread_view.rs](cci:7://file:///Users/dima/Developer/zed/crates/agent_ui/src/acp/thread_view.rs:0:0-0:0) - handles the UI for permission buttons
- **Permission Handling**: [/Users/dima/Developer/zed/crates/acp_thread/src/acp_thread.rs](cci:7://file:///Users/dima/Developer/zed/crates/acp_thread/src/acp_thread.rs:0:0-0:0) - manages permission requests in Zed
- **Agent Communication**: [/Users/dima/Developer/zed/crates/agent_servers/src/acp.rs](cci:7://file:///Users/dima/Developer/zed/crates/agent_servers/src/acp.rs:0:0-0:0) - handles communication with external agents
- **Settings**: [/Users/dima/Developer/zed/crates/agent_settings/src/agent_settings.rs](cci:7://file:///Users/dima/Developer/zed/crates/agent_settings/src/agent_settings.rs:0:0-0:0) - contains `always_allow_tool_actions` setting

### Current Settings:

There's already a global setting `always_allow_tool_actions` in [AgentSettings](cci:2://file:///Users/dima/Developer/zed/crates/agent_settings/src/agent_settings.rs:23:0-51:1) that can automatically allow tool actions without prompting, but this is an all-or-nothing setting and doesn't provide per-command whitelist/blacklist functionality.

## Answer to Your Question

**Can ACP have a whitelist/blacklist in settings of CLI commands?**

Currently, **no**. ACP does not have a built-in whitelist/blacklist system in Zed's settings for specific CLI commands. The "Allow always" functionality is handled entirely by the external agent servers, not by Zed itself.

To implement this feature, you would need to:

1. **Add new settings** to [AgentSettings](cci:2://file:///Users/dima/Developer/zed/crates/agent_settings/src/agent_settings.rs:23:0-51:1) for command whitelists/blacklists
2. **Modify the permission logic** in [acp_thread.rs](cci:7://file:///Users/dima/Developer/zed/crates/acp_thread/src/acp_thread.rs:0:0-0:0) to check these settings before prompting
3. **Update the UI** to show when commands are auto-allowed/blocked based on these settings
4. **Store the command patterns** in Zed's settings database







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

## Support external agent history

PR closed because ACP does not support history yet.

https://github.com/zed-industries/zed/pull/45734

# agent: History and recent conversations persistence per workspace

I did not check this out.

https://github.com/zed-industries/zed/pull/41874 (PR closed)

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
