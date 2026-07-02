# About

_Last updated: 2026-07-02 (Stage 5 complete + iteration round: deterministic clarify/search chat flow, activity-based "For You" tab, platform icon dedup — known UI/UX bugs deferred to Stage 5.5)_

## What It Is

Game Backlog Manager — a Windows-first desktop app (Tauri + React) for tracking a personal game backlog, auto-logging playtime, getting AI-driven "what to play next" suggestions, and saving gameplay clips via a hotkey. Single-user, local-first.

## Stack

- **Framework:** Tauri v2 (Rust backend + WebView frontend)
- **Frontend:** React 18 + Vite + TypeScript, Tailwind CSS v3 (via PostCSS, not the v4 Vite plugin), Zustand (state), React Router (`HashRouter`, safe under Tauri's asset protocol), Recharts (Dashboard charts)
- **Backend (Rust):** `rusqlite` (bundled SQLite), `sysinfo` (process polling), `reqwest`/`tokio` (HTTP/async), `tauri-plugin-global-shortcut`, `tauri-plugin-shell`, `tauri-plugin-sql`
- **External services:** RAWG API (game metadata + per-game details, key bundled server-side in Rust), Cloudflare Worker (`proxy/`) proxying Groq's chat completions API (AI recommendation chat) — deployed to a `*.workers.dev` subdomain; the worker URL is a bundled Rust const in `commands.rs` (same pattern as the RAWG key), not user-configurable
- **Media:** bundled `ffmpeg` binary for rolling-buffer screen capture and clip extraction
- **Deployment:** GitHub Actions builds `.msi`/`.dmg` installers on `v*` git tags, published to GitHub Releases
- **Dev environment:** developed on macOS, ships for Windows — Windows-only pieces (`gdigrab` capture, registry-based launch-on-startup, `.msi` build) can't be exercised locally and are validated via the GitHub Actions `windows-latest` runner

## Structure

```
src-tauri/src/    main.rs/lib.rs (entry, invoke_handler), db.rs (schema init + migrations via rusqlite),
                  commands.rs (Tauri commands), rawg.rs/tracker.rs (implemented, Stage 2/3/4/5), clipper.rs (stub for Stage 6)
src/components/   Layout (Sidebar, Shell), Backlog (grouped by status with contextual action
                  buttons — no status dropdown, wired to backend), Dashboard (playtime/stats only —
                  no longer shows AI recommendations, moved out), Search (RAWG search +
                  add-to-backlog + expandable GameCard, wired to backend), Recommendations
                  (two tabs: "For You" — activity-based via get_dashboard_recommendations, and
                  "Chat" — conversational game matching via chat_recommend, both rendering
                  GameCard grids), shared/GameCard.tsx (compact card + expand-to-detail, used by
                  Search and Recommendations), Clips, Settings (stub again — no worker-URL field,
                  it's hardcoded in Rust; capture/tray settings still stub)
src/store/        useAppStore.ts — Zustand store (games list, currentlyPlaying)
proxy/            Cloudflare Worker (Stage 5) — src/index.ts proxies chat turns to Groq's API,
                  deployed to a workers.dev subdomain via `wrangler deploy`
```

## Key Files

| File | Purpose |
|------|---------|
| `src-tauri/src/db.rs` | SQLite schema (games, sessions, clips, recommendations, settings) + `init()` called from `lib.rs` setup hook. `add_column_if_missing` runs lightweight migrations for columns added after initial release (e.g. `sessions.last_seen_at`, `sessions.ended_estimated`) without breaking existing installs. |
| `src-tauri/src/lib.rs` | Tauri builder: registers plugins (opener, shell), runs `db::init`, manages `DbState`, wires `invoke_handler` |
| `src-tauri/src/commands.rs` | `#[tauri::command]` functions: `get_backlog`, `search_rawg`, `add_game` (upsert by rawg_id), `update_game_status`, `delete_game` (transactional — deletes dependent `sessions`/`clips` rows before the `games` row, or the FK constraint rejects the delete for any game with tracked playtime), `set_game_exe_name`, `get_dashboard_stats(period)` (status counts + period-scoped total playtime + per-game playtime with cover art + last-7-days chart data; `period` is `"day"\|"week"\|"month"\|"all"`), `get_currently_playing` (reads the live open session straight from the DB, not from events — avoids a race with the tracker's startup reconciliation), `get_playtime_totals` (all-time playtime per game, for Backlog's inline display), `get_game_details(rawg_id)` (thin wrapper over `rawg::get_game_details`, used by `GameCard`'s expand), `chat_recommend(message, history, awaiting_answer)` (Stage 5 — POSTs to the bundled `WORKER_URL` const; `awaiting_answer` is set by the frontend, not inferred, telling the worker whether this message answers a pending clarifying question (search now) or starts a new ask (clarify first) — on a `{type:"search"}` reply, resolves each named title individually via `rawg::resolve_titles` so the RAWG key never leaves this binary; best-effort logs each turn to `recommendations`), `get_dashboard_recommendations` (activity-based "For You" tab — picks the user's top-played genre + 3 favorite games via `compute_genre_signal`, POSTs to the worker's `/suggest` endpoint, and caches the result in `dashboard_recommendations_cache` — only regenerates when backlog size, top genre, or that genre's playtime shift meaningfully, or 5 hours pass, so opening the tab repeatedly doesn't burn an AI call every time). All structs serialize camelCase to match frontend. |
| `src-tauri/src/rawg.rs` | RAWG API key (bundled const) + `search_games()` (lightweight `RawgGameResult` — id, name, cover, genre, platform, `rawgUrl`, used for grid cards), `get_game_details(rawg_id)` (hits `/games/{id}` for the richer `RawgGameDetail` — description, Metacritic, developer, publisher, website — fetched lazily on card expand), and `resolve_titles(titles, max)` (looks up a list of AI-named specific game titles individually via `search_best_match`, dedup'd by `rawg_id` — used instead of a single keyword search so chat/dashboard recommendations get real variety instead of RAWG's title-search surfacing a wall of same-title reskins for one generic query) |
| `src-tauri/src/tracker.rs` | `start(app)` spawns a tokio task (via `setup`) that: (1) runs `reconcile_dangling_sessions` once at startup to resolve sessions left open by an unclean previous shutdown (crash/force-quit/OS restart) — re-adopts still-running games, otherwise closes them out using the `last_seen_at` heartbeat and flags `ended_estimated`; (2) each poll, runs `detect_unregistered_games` + `auto_register_and_track` to auto-add and start tracking any game launched from a known storefront path (`steamapps/common`, Epic/GOG/Battle.net/Riot folders) that isn't in the backlog yet — the raw folder/exe name is run through `humanize_name` (splits camelCase/acronym/letter-digit boundaries, e.g. `"BloonsTD6"` → `"Bloons TD 6"`) before it's used as both the RAWG search query and the display-name fallback, since storefronts often use unspaced folder names that don't match RAWG's listed titles — best-effort enriched via RAWG (preferring an exact case-insensitive name match over just the first result), emitting `game-auto-added`; (3) polls `sysinfo` every 5s, matches running process names against `games.exe_name`, opens/closes `sessions` rows (writing a `last_seen_at` heartbeat on every poll of an active session) — on open, also flips `games.status` to `playing` unless it's `completed`/`dropped` (a deliberate user call that a relaunch shouldn't silently undo) — and emits `session-started`/`session-ended` events to the frontend |
| `src/App.tsx` | React Router route table under the `Shell` layout |
| `src/components/Layout/Sidebar.tsx` | Nav: Dashboard/Backlog/Search/Recommendations/Clips/Settings |
| `src/store/useAppStore.ts` | `Game` type mirrors the `games` table; Zustand store for games + currentlyPlaying |
| `src-tauri/tauri.conf.json` | Window sized 1200x800 (min 900x600), bundle targets `["msi","nsis","dmg"]` |
| `src/components/shared/GameCard.tsx` | Compact grid card (cover, name, up to 2 genre chips, platform-category icons, RAWG link) that expands on click into a detail overlay (lazy-fetches `get_game_details`); shared by `Search.tsx` and `Recommendations.tsx` so metadata/expand behavior lives in one place. `platformCategories()` collapses RAWG's many individual platform entries (e.g. "PlayStation 4", "PlayStation 5", "Xbox Series S/X") into at most 4 deduped icons — Windows, Mac, Console, Mobile — instead of one icon per entry. |
| `proxy/src/index.ts` | Cloudflare Worker with two POST endpoints: `/chat` (used by `chat_recommend`) and `/suggest` (used by `get_dashboard_recommendations`). `/chat`'s clarify-vs-search branch is decided by the request's `awaitingAnswer` field, not left to the model — `CLARIFY_SYSTEM_PROMPT` (asks exactly one clarifying question, reasoning about what the request *doesn't* yet specify rather than a canned question) and `SEARCH_SYSTEM_PROMPT` (names 4-8 specific titles) are two separate calls picked deterministically in code. `/suggest` takes a favorites list and always returns titles+reasoning directly (no clarify branch — it's non-interactive). CORS on every response. `GROQ_API_KEY` is a `wrangler secret`, typed via declaration merging in `proxy/src/env.d.ts` (not visible to `wrangler types` since it's a secret, not a config var) |

## Common Tasks

- **Add a Tauri command:** implement in `commands.rs`, register in `lib.rs`'s `invoke_handler![...]`, call from React via `@tauri-apps/api/core` `invoke()`.
- **Add a DB table/column:** update `SCHEMA` const in `db.rs`, add query logic in a command, expose it.
- **Add a new view/route:** create `src/components/<Area>/<Area>.tsx`, add a `<Route>` in `App.tsx`, add a `NavLink` in `Sidebar.tsx`.

## Conventions & Gotchas

- RAWG key must live as a Rust constant in `rawg.rs` — never expose it in the frontend bundle.
- Use `tauri-plugin-sql` only from the frontend for simple reads; use `rusqlite` directly in Rust (already wired via `DbState`) for anything with joins/aggregates.
- The tracker loop (Stage 3) must never block the main thread — use `tokio::spawn`.
- `sysinfo` returns `.exe`-suffixed names on Windows but extension-less names on Mac — `tracker.rs::normalize_exe_name` lowercases and strips `.exe` on both sides of the comparison.
- SQLite runs in WAL mode with `synchronous = NORMAL` (set in `db::init`) so the local DB survives app crashes/kills without the fsync cost of `FULL`; the file lives in the OS app-data dir, so it persists across restarts and app updates untouched.
- ffmpeg capture flag differs by OS: `-f gdigrab` (Windows) vs `-f avfoundation` (Mac) — branch in `clipper.rs`.
- Cloudflare Worker must return CORS headers on every response or the Tauri WebView's fetch will fail.
- The Worker proxies to **Groq** (OpenAI-compatible chat completions), not Ollama — no self-hosted model to babysit; free-tier rate limits are generous enough for a single-user app. `GROQ_API_KEY` lives only as a Worker secret (`wrangler secret put`), never in the repo or frontend.
- The Worker's own URL is a hardcoded Rust const (`WORKER_URL` in `commands.rs`) — same pattern as the RAWG key, deliberately not user-configurable (an earlier version stored it in `settings` with a Settings UI field; reverted per user request to hide the detail from end users).
- Never let the model self-judge "should I clarify or search now" from prose instructions alone — it doesn't reliably comply (tried, it just searched immediately anyway). The chat flow is instead deterministic in code: the frontend explicitly tells the worker `awaitingAnswer` (true if this message answers a clarifying question just shown, false if it's a new ask), and the worker picks between two entirely separate system prompts based on that flag. Apply the same pattern for any future multi-step AI flow in this app rather than trusting prompt engineering alone.
- Chat history is never truncated/reset between asks (it used to be, to stop an unrelated new ask from inheriting old context — but the user wants continued, deepening conversations instead) — correctness now comes from the explicit `awaitingAnswer` flag, not from wiping history.
- RAWG's search endpoint (`/games`) does not reliably return `description_raw`/`metacritic`/`developers`/`publishers`/`website` — those only come from the per-game detail endpoint (`/games/{id}`), which is why `get_game_details` is a separate on-demand call (triggered by `GameCard`'s expand) rather than folded into `search_games`.
- When asking Groq to recommend games, have it name specific titles it knows (an array of names), then resolve each individually via RAWG (`rawg::resolve_titles`) — a single keyword-search query against RAWG's title index surfaces shovelware clones (e.g. "farming simulation" → a wall of "Farming Simulator N" reskins) instead of genuinely varied results.
- Real-time updates (session start/end, clip saved) go Rust → frontend via `tauri::Emitter` + `listen()`, not polling. Treat these events as "refresh now" triggers only, not sources of truth on load — always fetch live/authoritative state (e.g. `get_currently_playing`) on mount too, since events emitted before a listener attaches are lost (no buffering), which matters especially for the tracker's startup reconciliation pass.
- SQLite's `CURRENT_TIMESTAMP` / `julianday()` are UTC but rendered without a timezone marker (`"YYYY-MM-DD HH:MM:SS"`). When parsing these in the frontend, append `"Z"` (after replacing the space with `"T"`) before `new Date(...)`, or JS silently treats them as local time.
- Auto-add-to-backlog is a path heuristic, not an authoritative library list: it only catches games that have actually been launched at least once, and only ones installed under a recognized storefront folder (`steamapps/common`, `Epic Games`, `GOG Games`, `Battle.net`, `Riot Games`). It cannot see games that have never been run, or that live outside those folder conventions — that gap is intentionally left for the future Steam-account sign-in import (Stage 10, plan task 17), not patched with more heuristics.
- RAWG calls inside `tracker.rs` must happen *before* acquiring `DbState`'s mutex, never while holding it — the connection is a plain `std::sync::Mutex`, and holding its guard across an `.await` blocks every other command trying to touch the DB for the duration of the network call.
- `foreign_keys = ON` is set in `db::init`, so deleting a row that other tables reference (e.g. a `games` row with `sessions`/`clips` rows pointing at it) fails with a foreign key constraint error unless dependents are deleted first — see `delete_game`'s transaction for the pattern to follow when adding new delete commands.
- A storefront's install-folder name often doesn't match RAWG's listed title verbatim (unspaced, abbreviated, etc.) — run it through `tracker::humanize_name` before searching RAWG rather than searching the raw name directly.
- Backlog has no manual status dropdown (removed per user feedback — nobody wants to babysit a select box). Status is driven by the tracker on session start (→ `playing`, unless `completed`/`dropped`) plus a small set of contextual action buttons (`StatusActions` in `Backlog.tsx`) matched to the game's current status. If you add a new status or transition, update `StatusActions`'s switch, not a shared dropdown.
- Tailwind is pinned to v3 (PostCSS-based) intentionally, per project plan — do not upgrade to v4/`@tailwindcss/vite` without discussion.
- The marketing website is a **separate repo**, not part of this one — this repo is app-only plus a future `proxy/` folder.
