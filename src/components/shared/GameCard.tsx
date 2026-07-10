import { useState } from "react";
import type { ReactNode, MouseEvent } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, Loader2, X, Monitor, Apple, Smartphone, Gamepad2 } from "lucide-react";
import { CoverImage } from "./CoverImage";

export interface RawgGameResult {
  rawgId: number;
  name: string;
  coverUrl: string | null;
  genre: string | null;
  platform: string | null;
  rawgUrl: string;
}

export interface RawgGameDetail extends RawgGameResult {
  description: string | null;
  metacriticScore: number | null;
  developer: string | null;
  publisher: string | null;
  websiteUrl: string | null;
}

type PlatformCategory = "windows" | "mac" | "console" | "mobile";

const PLATFORM_CATEGORY_ORDER: PlatformCategory[] = ["windows", "mac", "console", "mobile"];

const PLATFORM_CATEGORY_META: Record<PlatformCategory, { label: string; icon: typeof Monitor }> = {
  windows: { label: "Windows", icon: Monitor },
  mac: { label: "Mac", icon: Apple },
  console: { label: "Console", icon: Gamepad2 },
  mobile: { label: "Mobile", icon: Smartphone },
};

function categorizePlatform(name: string): PlatformCategory | null {
  const n = name.toLowerCase();
  if (n.includes("ios") || n.includes("android")) return "mobile";
  if (n.includes("mac")) return "mac";
  if (n.includes("pc") || n.includes("windows") || n.includes("linux")) return "windows";
  return "console";
}

// RAWG lists platforms individually (e.g. "PlayStation 4", "PlayStation 5", "Xbox Series S/X"),
// which used to render a repeated icon per entry — collapse to at most one icon per category.
function platformCategories(platform: string | null): PlatformCategory[] {
  const present = new Set(splitList(platform).map(categorizePlatform).filter((c): c is PlatformCategory => c !== null));
  return PLATFORM_CATEGORY_ORDER.filter((c) => present.has(c));
}

function splitList(value: string | null, limit?: number): string[] {
  if (!value) return [];
  const items = value.split(", ").filter(Boolean);
  return limit ? items.slice(0, limit) : items;
}

function metacriticClass(score: number): string {
  if (score >= 75) return "bg-success/15 text-success";
  if (score >= 50) return "bg-warning/15 text-warning";
  return "bg-danger/15 text-danger";
}

interface GameCardProps {
  game: RawgGameResult;
  /** Short per-game context line (e.g. the AI's "why this fits" note), shown under the genres. */
  note?: string | null;
  footer?: ReactNode;
}

/** Compact card for grids (Discover search + AI results); click anywhere to expand
 *  into a detail panel that lazily fetches richer RAWG metadata via `get_game_details`. */
export function GameCard({ game, note, footer }: GameCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [detail, setDetail] = useState<RawgGameDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function openDetail() {
    setExpanded(true);
    if (detail || loading) return;
    setLoading(true);
    setError(null);
    try {
      const d = await invoke<RawgGameDetail>("get_game_details", { rawgId: game.rawgId });
      setDetail(d);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  function stop(e: MouseEvent) {
    e.stopPropagation();
  }

  function openRawg(e: MouseEvent) {
    e.stopPropagation();
    e.preventDefault();
    void openUrl(game.rawgUrl);
  }

  const genres = splitList(game.genre, 2);
  const platforms = platformCategories(game.platform);

  return (
    <>
      <div
        onClick={openDetail}
        role="button"
        tabIndex={0}
        onKeyDown={(e) => {
          // The card is a div (it nests real buttons), so Enter/Space activation is manual.
          // Only when the card itself is focused — key events from the nested buttons bubble here.
          if (e.target !== e.currentTarget) return;
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            void openDetail();
          }
        }}
        className="group flex h-full cursor-pointer select-none flex-col overflow-hidden rounded-xl border border-border bg-surface transition-all duration-200 hover:-translate-y-0.5 hover:border-accent/40 hover:shadow-[0_16px_32px_-16px_rgba(0,0,0,0.6)]"
      >
        <CoverImage src={game.coverUrl} alt={game.name} className="h-32 w-full shrink-0">
          <div className="absolute inset-0 bg-gradient-to-t from-surface/70 via-transparent to-transparent" />
        </CoverImage>
        <div className="flex flex-1 flex-col p-3">
          <div className="flex items-start justify-between gap-1">
            <p className="truncate text-[13.5px] font-semibold text-text-hi">{game.name}</p>
            <button
              onClick={openRawg}
              title="View on RAWG"
              className="shrink-0 text-text-lo/70 transition-colors hover:text-accent"
            >
              <ExternalLink className="h-3.5 w-3.5" />
            </button>
          </div>

          {genres.length > 0 && (
            <div className="mt-1.5 flex flex-wrap gap-1">
              {genres.map((g) => (
                <span
                  key={g}
                  className="rounded-full bg-surface-alt px-2 py-0.5 text-[10.5px] font-medium text-text-lo"
                >
                  {g}
                </span>
              ))}
            </div>
          )}

          {platforms.length > 0 && (
            <div className="mt-2 flex gap-1.5 text-text-lo/70">
              {platforms.map((cat) => {
                const { icon: Icon, label } = PLATFORM_CATEGORY_META[cat];
                return <Icon key={cat} className="h-3.5 w-3.5" aria-label={label} />;
              })}
            </div>
          )}

          {note && (
            <p className="mt-2 text-[11.5px] italic leading-snug text-text-lo">{note}</p>
          )}

          {footer && (
            <div onClick={stop} className="mt-auto pt-3">
              {footer}
            </div>
          )}
        </div>
      </div>

      {expanded &&
        createPortal(
          <div
            className="fixed inset-0 z-50 flex items-center justify-center bg-bg/80 p-4 backdrop-blur-sm animate-fade-in"
            onClick={() => setExpanded(false)}
          >
          <div
            onClick={stop}
            className="max-h-[85vh] w-full max-w-lg animate-fade-up overflow-y-auto rounded-2xl border border-border bg-surface p-6 shadow-2xl"
          >
            <div className="flex items-start justify-between gap-3">
              <h2 className="page-title text-xl">{game.name}</h2>
              <button
                onClick={() => setExpanded(false)}
                className="shrink-0 rounded-full p-1 text-text-lo transition-colors hover:bg-surface-alt hover:text-text-hi"
              >
                <X className="h-5 w-5" />
              </button>
            </div>

            {game.coverUrl && (
              <CoverImage
                src={game.coverUrl}
                alt={game.name}
                eager
                className="mt-4 h-40 w-full rounded-xl"
              />
            )}

            {loading && (
              <div className="mt-5 flex items-center gap-2 text-sm text-text-lo">
                <Loader2 className="h-4 w-4 animate-spin" /> Loading details...
              </div>
            )}
            {error && <p className="mt-4 text-sm text-danger">{error}</p>}

            {detail && (
              <div className="mt-5 space-y-4">
                <div className="flex flex-wrap items-center gap-2 text-xs">
                  {detail.metacriticScore != null && (
                    <span
                      className={`rounded-full px-2 py-0.5 font-semibold ${metacriticClass(detail.metacriticScore)}`}
                    >
                      Metacritic {detail.metacriticScore}
                    </span>
                  )}
                  {splitList(detail.genre).map((g) => (
                    <span
                      key={g}
                      className="rounded-full bg-surface-alt px-2 py-0.5 font-medium text-text-lo"
                    >
                      {g}
                    </span>
                  ))}
                </div>

                <div className="flex flex-wrap items-center gap-3 text-xs text-text-lo">
                  {platformCategories(detail.platform).map((cat) => {
                    const { icon: Icon, label } = PLATFORM_CATEGORY_META[cat];
                    return (
                      <span key={cat} className="flex items-center gap-1">
                        <Icon className="h-3.5 w-3.5" /> {label}
                      </span>
                    );
                  })}
                </div>

                {detail.description && (
                  <p className="max-h-40 overflow-y-auto text-sm leading-relaxed text-text-lo">
                    {detail.description}
                  </p>
                )}

                {(detail.developer || detail.publisher) && (
                  <div className="grid grid-cols-2 gap-3 rounded-xl bg-surface-alt/60 p-3 text-sm">
                    {detail.developer && (
                      <div>
                        <p className="text-[11px] uppercase tracking-wide text-text-lo/70">
                          Developer
                        </p>
                        <p className="mt-0.5 text-text-hi">{detail.developer}</p>
                      </div>
                    )}
                    {detail.publisher && (
                      <div>
                        <p className="text-[11px] uppercase tracking-wide text-text-lo/70">
                          Publisher
                        </p>
                        <p className="mt-0.5 text-text-hi">{detail.publisher}</p>
                      </div>
                    )}
                  </div>
                )}

                <button
                  onClick={openRawg}
                  className="inline-flex items-center gap-1.5 text-sm font-medium text-accent transition-colors hover:text-accent-hover"
                >
                  View on RAWG <ExternalLink className="h-3.5 w-3.5" />
                </button>
              </div>
            )}

            {footer && <div className="mt-5">{footer}</div>}
          </div>
        </div>,
          document.body,
        )}
    </>
  );
}
