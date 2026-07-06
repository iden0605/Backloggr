# Release Workflow & Shipping Pipeline

## Overview

How a commit on `dev` becomes a public installer on GitHub Releases, and what gets baked into the Windows build along the way (bundled ffmpeg, native game audio). Introduced in task 16 — before this, the repo had CI checks but no way to produce an installer.

## What Changed

- **`.github/workflows/release.yml`** (new) — builds `.msi`/`.nsis` (Windows) and `.dmg` (macOS) installers when a `v*` tag is pushed, and attaches them to a **draft** GitHub Release.
- **`src-tauri/tauri.windows.conf.json`** (new) — bundles ffmpeg into the Windows installer as a Tauri sidecar (`bundle.externalBin`), so end users never install ffmpeg themselves.
- **`src-tauri/src/loopback.rs`** (new) — native WASAPI loopback capture: game/system audio in clips with zero user setup on Windows.
- **`clipper.rs::ffmpeg_path()`** — resolves the bundled sidecar ffmpeg next to the app executable, falling back to PATH (dev builds, macOS).

## How Releases Work With `dev` and `main`

The workflows split by trigger:

| Workflow | Trigger | Purpose |
|----------|---------|---------|
| `windows-check.yml` | every push/PR to `dev` or `main` | `cargo check` on a Windows runner — catches broken Windows-only Rust that macOS development never compiles |
| `release.yml` | push of a tag matching `v*` | builds installers, creates a draft GitHub Release |

`release.yml` is **tag-triggered, not branch-triggered** — a tag points at a commit, so the branch it sits on doesn't matter to the workflow. The intended flow:

```bash
# 1. Day-to-day work lands on dev (windows-check validates every push)

# 2. Ready to ship: sync versions, merge to main
#    (bump "version" in src-tauri/tauri.conf.json and package.json first —
#    the installer metadata comes from tauri.conf.json, not the tag name)
git checkout main
git merge dev
git push origin main

# 3. Tag the merge commit and push the tag — this is what fires release.yml
git tag v0.1.0
git push origin v0.1.0
```

The release lands as a **draft**: nothing is public until you review the attached installers on the GitHub Releases page and click publish. A broken build can be thrown away safely:

```bash
# delete the draft on GitHub, then remove the tag and re-cut it
git push origin :refs/tags/v0.1.0
git tag -d v0.1.0
```

For a pipeline shakeout it's fine to tag a `dev` commit directly — the workflow doesn't care.

## The ffmpeg Sidecar (Windows)

`tauri.windows.conf.json` declares `bundle.externalBin: ["binaries/ffmpeg"]`. Platform config files auto-merge only when building **on** that OS, so macOS dev never needs a sidecar file and keeps using Homebrew ffmpeg from PATH.

At build time, Tauri expects the real binary at `src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe` (the directory is gitignored). Who provides it:

- **`release.yml`** downloads a pinned static build (`ffmpeg-8.0.1-essentials_build.zip` from gyan.dev — versioned gyan URLs are permanent, unlike BtbN's rotating "latest" assets) and stages `bin/ffmpeg.exe` under that name.
- **`windows-check.yml`** stages an **empty placeholder** — tauri-build requires every `externalBin` file to exist even for `cargo check`. Any new workflow that compiles the Windows build needs the same step.

At runtime, `clipper.rs::ffmpeg_path()` looks for `ffmpeg(.exe)` next to the app executable (where Tauri installs sidecars) and falls back to a PATH lookup. All ffmpeg invocations go through it.

## Native Game Audio (WASAPI Loopback)

Shipped Windows builds capture game/system audio with **zero setup** — no Stereo Mix, no virtual audio device. `loopback.rs` opens a cpal *input* stream on the default *output* device (WASAPI's loopback mode — whatever the user hears through any headphones/speakers) and pipes raw PCM into the capture ffmpeg's stdin as an extra input, mixed with the mic by the existing `amix` path. The dshow loopback-device detection remains only as a fallback when the WASAPI stream can't open.

Two WASAPI constraints shape the implementation — keep them in mind before touching it:

- The audio callback runs on the engine's thread and must never block: samples hop through a bounded channel (`try_send`, drops on overflow) to a writer thread that owns the pipe.
- Loopback delivers **no packets while the machine is silent**, so the writer paces total bytes against the wall clock and pads with silence — without this, the clip's audio track drifts against the video.

The feed thread is deliberately detached: killing the capture ffmpeg breaks the pipe, and the broken pipe is the thread's exit signal. There is no handle to store and no shutdown ordering to get wrong.

## Caveats

- `release.yml` has not yet had a real tag run — the first `v*` push is the shakeout. Since releases are drafts, a broken run publishes nothing.
- The entire Windows runtime path (WASAPI pacing, gdigrab capture, installer behavior) is compile-verified only; it needs validation on real Windows hardware.
- The macOS `.dmg` exists for the dev platform: no bundled ffmpeg, and game audio still requires BlackHole + a Multi-Output Device.
