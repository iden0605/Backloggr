---
name: commit-and-push
description: Stage all changes, draft a commit message from the diff, confirm with the user, then push. Use when the user wants to commit and push their current changes.
---

You are about to stage, commit, and push all current changes. Follow these steps precisely:

1. Run `git status` to see what files have changed.
2. Run `git diff HEAD` to read through all changes since the last commit.
3. Based on the diff, draft a concise, accurate commit message. Follow conventional commit style if the repo already uses it, otherwise use a short imperative sentence. Do not guess — base the message entirely on what you read in the diff.
4. Run `git branch --show-current` to get the current branch name.
5. Present the user with:
   - The commit message you drafted
   - The branch you will push to
   - Ask them to confirm or suggest changes before proceeding.
6. Once the user confirms, stage all files with `git add -A` and commit using the agreed message.
7. Push to the remote with `git push origin <branch>`.
8. Report the result.

Do not stage, commit, or push until the user has explicitly confirmed.
