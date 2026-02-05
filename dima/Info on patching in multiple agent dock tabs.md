# Most important PR which contains the multiple agent tab implementation

<https://github.com/wzulfikar/zed/pull/8>

# Restore the usual suspects on heavy merge conflicts

```bash
git restore --source=main crates/agent_ui/src/agent_panel.rs crates/agent_ui/src/agent_ui.rs crates/agent_ui/src/acp/thread_view.rs
```

crates/agent_ui/src/agent_panel.rs
crates/agent_ui/src/agent_ui.rs
crates/agent_ui/src/acp/thread_view.rs

# How to apply via AI

Use `qwen` CLI to apply the diff and then use `amp` to brush over when `qwen` gets stuck or makes mistakes.

There was a time where <https://jules.google/> had Gemini 2.5 Pro for free and it was able to apply the diff correctly.
Nowadays, it uses Gemini 3 Flash which simply fails this.
Especially because the environment on jules can not fully compile as some dependencies are missing.