---
name: zed-create-gh-pr-and-merge
description: Push current branch via git, then use gh to create and merge a pull request after CI is passing
disable-model-invocation: true
---

# Zed Create GH PR and Merge

First note that this is a custom GitHub fork of the main Zed repository.

The main Zed repository is located at <https://github.com/zed-industries/zed>, while the fork is located at <https://github.com/Dima-369/zed>.

Never create a pull request against the main repository directly. Always target the fork and merge into the `dima` branch.

# When to use

When the user invokes this skill, without asking the user back, you should proceed with the following steps listed below:

# How to use

If you are on branch `dima`, create a new `fix/...` or `feat/...` git branch first. Check the `git diff` to figure out a good git branch name.

Proceed to commit the worktree changes, if there are any changes to commit.

---

Then if the git branch is not pushed yet, use `git push` to push it to the remote repository.

Next, create a GitHub pull request using the `gh pr create` command and invoke the CI checks and merge commands in one go:

```bash
# --fill automates title/body
# take note of the PR number
gh pr create --base dima --fill --repo Dima-369/zed

gh pr checks {PR number from above} --watch && gh pr merge --squash --delete-branch
```
