---
name: version-update
description: Use when someone asks to bump the version, prepare a release, cut a new version, update the app version, or asks what the next version number should be.
argument-hint: [optional explicit version, e.g. 0.3.0]
disable-model-invocation: true
---

## What This Skill Does

Prepares a backloggr release end-to-end short of publishing: reports the current version, recommends the next semver number from the real changes since the last release, syncs every version file in the repo, then commits, tags, and pushes — which triggers `release.yml` to build installers into a DRAFT release on the public `iden0605/backloggr-releases` repo. **Publishing that draft on GitHub stays manual** (and the pre-release checkbox must stay unticked, or the website's download button won't see it).

## Context

- `release.yml` stamps the app version **from the git tag** at build time (`v0.3.0` → installers versioned `0.3.0`). The repo's own version files only affect dev builds — but this skill keeps them in sync anyway so nothing drifts.
- Version files: `src-tauri/tauri.conf.json` (`version`), `src-tauri/Cargo.toml` (`[package] version`), `package.json` (`version`), plus their lockfiles.
- The website's download button serves the latest **full (non-pre-release)** release from `iden0605/backloggr-releases` automatically — no website work is ever needed here.

## Steps

1. **Report the current version.** Gather and show the user, clearly labeled:
   - Latest public release: `gh release list -R iden0605/backloggr-releases --limit 3` (note draft/pre-release markers)
   - Latest tag in this repo: `git tag --sort=-v:refname | head -3`
   - Repo file versions: read `version` from `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `package.json` — flag any disagreement between them (expected historical drift; this run fixes it).

2. **Check the working tree and branch.** Run `git status --short` and `git branch --show-current`. If there are uncommitted changes, tell the user and ask whether to (a) commit them separately first themselves, (b) have them included in this release's history before tagging, or (c) abort. Never tag with unresolved uncommitted changes silently. **Releases are cut from `main` ONLY** (user rule): if not on `main`, offer to merge the current branch into `main` (`git checkout main && git pull && git merge <branch> && git push`) and continue there, or abort — never tag `dev` or a feature branch.

3. **Analyze what changed** since the latest tag: `git log <latest-tag>..HEAD --oneline` (if no tag exists, summarize recent history instead). Classify the commits:
   - Only fixes/chores/docs → **patch** bump
   - Any new feature or notable behavior change → **minor** bump
   - Breaking/major rework → still minor while pre-1.0, but say so
   - `1.0.0` is never auto-recommended — it's the user's deliberate "ready for strangers" call.

4. **Recommend and confirm.** Present the recommendation with a one-line reason per significant commit group, then confirm via AskUserQuestion (recommended version first, the other bump size as an alternative; the user can type a custom one via Other). If the user passed an explicit version as `$ARGUMENTS`, validate it's semver, higher than the current latest tag, and propose that instead — still confirm before writing.

5. **Sync version files** to the confirmed version:
   - `src-tauri/tauri.conf.json` → `version`
   - `package.json` → `version`, then `npm install --package-lock-only` to sync the lockfile
   - `src-tauri/Cargo.toml` → `[package] version`, then run `cargo metadata --format-version 1 > /dev/null` inside `src-tauri/` to sync `Cargo.lock` without a compile
   - Re-read all three to verify they agree.

6. **Commit, tag, push.**
   - Commit message: `Chore: bump version to <X.Y.Z>` — no Co-Authored-By trailer (user preference).
   - Refuse to proceed if `git tag -l v<X.Y.Z>` already exists locally or `git ls-remote --tags origin v<X.Y.Z>` shows it on the remote.
   - `git push`, then `git tag v<X.Y.Z>` and `git push origin v<X.Y.Z>`. Never force-push anything.

7. **Hand off.** Tell the user:
   - The release build is running: give the Actions URL (`gh run list --workflow release.yml --limit 1` for the live run).
   - When it finishes (~20 min), review the draft at https://github.com/iden0605/backloggr-releases/releases and **publish it with pre-release UNTICKED**.
   - The website download button updates itself the moment the draft is published.

## Notes

- This skill never publishes the GitHub release and never touches the website — publishing is the user's manual safety gate by design.
- If `gh` calls fail with auth/scope errors, surface the exact error and stop; don't guess versions from partial data.
- If the release workflow's `RELEASES_TOKEN` PAT has expired (upload step fails 401/403 on a previous run), remind the user to regenerate it and `gh secret set RELEASES_TOKEN` — but don't block the version bump on it.
