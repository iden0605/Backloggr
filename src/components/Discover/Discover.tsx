import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useLocation, useNavigate } from "react-router-dom";
import { Search as SearchIcon, Loader2, SearchX, Sparkles, Compass } from "lucide-react";
import { GameCard, type RawgGameResult } from "../shared/GameCard";
import { EmptyState } from "../shared/EmptyState";
import { DotBounce } from "../shared/DotBounce";
import { AddToLibraryButton, useAddToLibrary, type ChatRecommendResponse } from "./common";
import { useAppStore } from "../../store/useAppStore";

/**
 * The merged find-a-game surface (task 20): one find box up top with two explicit
 * actions — Search (RAWG title lookup) and Ask Shelby (drops into the chat flow at
 * /discover/chat, carrying any typed text as the first message). "For You"
 * recommendations are the page's idle content; a search swaps them for the results
 * grid until cleared.
 */
export function Discover() {
  const navigate = useNavigate();
  const location = useLocation();
  const [query, setQuery] = useState("");
  // null = no active search (For You shows); [] = a search that found nothing.
  const [results, setResults] = useState<RawgGameResult[] | null>(null);
  const [searchedQuery, setSearchedQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { addedIds, addToLibrary, error: addError } = useAddToLibrary();
  const autoSearchedRef = useRef(false);

  async function searchFor(trimmed: string) {
    if (!trimmed || loading) return;
    setLoading(true);
    setError(null);
    try {
      const found = await invoke<RawgGameResult[]>("search_rawg", { query: trimmed });
      setResults(found);
      setSearchedQuery(trimmed);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  function runSearch(e: React.FormEvent) {
    e.preventDefault();
    void searchFor(query.trim());
  }

  // Quick-open (Ctrl+K) hands off "not in your library" queries as router state — run the
  // search once and clear the state so a remount/back can't rerun it.
  useEffect(() => {
    const state = location.state as { search?: string } | null;
    if (!state?.search || autoSearchedRef.current) return;
    autoSearchedRef.current = true;
    navigate(location.pathname, { replace: true, state: null });
    setQuery(state.search);
    void searchFor(state.search);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function clearSearch() {
    setResults(null);
    setSearchedQuery("");
    setQuery("");
  }

  function askAi() {
    navigate("/discover/chat", { state: { ask: query.trim() || undefined } });
  }

  return (
    <div>
      <h1 className="page-title text-[26px]">Discover</h1>
      <p className="mt-1.5 text-[13.5px] text-text-lo">
        Search for a title you know, or ask Shelby when you don't.
      </p>

      <form onSubmit={runSearch} className="mt-6">
        <div className="flex items-center gap-2 rounded-2xl border border-border bg-surface py-2 pl-4 pr-2 shadow-[0_8px_24px_-12px_rgba(0,0,0,0.5)] transition-colors focus-within:border-accent/45">
          <SearchIcon className="h-4 w-4 shrink-0 text-text-lo" />
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={'A title you know, or a mood — "cozy game for short sessions"...'}
            className="min-w-0 flex-1 bg-transparent py-1 text-sm text-text-hi outline-none placeholder:text-text-lo/70"
          />
          <button
            type="button"
            onClick={askAi}
            className="flex shrink-0 items-center gap-1.5 rounded-xl border border-border-strong px-3.5 py-2 text-[13px] font-semibold text-text-hi transition-all duration-150 hover:border-accent/50 hover:text-accent-hover active:scale-[0.98]"
          >
            <Sparkles className="h-3.5 w-3.5 text-accent" />
            Ask Shelby
          </button>
          <button
            type="submit"
            disabled={loading}
            className="flex shrink-0 items-center gap-2 rounded-xl bg-text-hi px-[18px] py-2 text-[13px] font-semibold text-bg transition-all duration-150 hover:opacity-85 enabled:active:scale-[0.98] disabled:opacity-50"
          >
            {loading && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {loading ? "Searching..." : "Search"}
          </button>
        </div>
        <p className="mt-2 text-center font-mono text-[10px] uppercase tracking-wide text-text-lo/50">
          Enter searches RAWG · Ask Shelby starts a conversation
        </p>
      </form>

      {(error || addError) && <p className="mt-4 text-sm text-danger">{error ?? addError}</p>}

      {results === null ? (
        <ForYouSection addedIds={addedIds} onAdd={addToLibrary} />
      ) : (
        <section className="mt-8 animate-fade-up">
          <div className="flex items-baseline gap-3">
            <p className="shelf-label">Results for &ldquo;{searchedQuery}&rdquo;</p>
            <button
              onClick={clearSearch}
              className="text-[12.5px] text-text-lo underline underline-offset-4 transition-colors hover:text-text-hi"
            >
              Clear
            </button>
          </div>
          {results.length === 0 ? (
            <div className="mt-6">
              <EmptyState icon={SearchX} title="No games found">
                Try a different title or check the spelling.
              </EmptyState>
            </div>
          ) : (
            <div className="mt-4 grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4">
              {results.map((game) => (
                <GameCard
                  key={game.rawgId}
                  game={game}
                  footer={
                    <AddToLibraryButton
                      game={game}
                      added={addedIds.has(game.rawgId)}
                      onAdd={addToLibrary}
                    />
                  }
                />
              ))}
            </div>
          )}
        </section>
      )}
    </div>
  );
}

function ForYouSection({
  addedIds,
  onAdd,
}: {
  addedIds: Set<number>;
  onAdd: (game: RawgGameResult) => void;
}) {
  const navigate = useNavigate();
  // The shown set lives in the store so Discover ↔ Shelby round-trips keep the exact same
  // grid (including "Load more" batches) — Shell clears it when the user leaves /discover*.
  const recs = useAppStore((s) => s.forYouRecs);
  const setRecs = useAppStore((s) => s.setForYouRecs);
  const [loading, setLoading] = useState(recs === null);
  const [loadingMore, setLoadingMore] = useState(false);
  // Set once a "Load more" round comes back with nothing new — hides the button rather
  // than letting it spin up empty AI calls forever.
  const [exhausted, setExhausted] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (recs !== null) return;
    invoke<ChatRecommendResponse>("get_dashboard_recommendations")
      .then((res) => {
        if (res.type === "results") setRecs({ reasoning: res.reasoning, games: res.games });
      })
      .catch((err) => setError(String(err)))
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function loadMore() {
    if (!recs || loadingMore) return;
    setLoadingMore(true);
    setError(null);
    try {
      const res = await invoke<ChatRecommendResponse>("get_more_dashboard_recommendations", {
        shown: recs.games.map((g) => g.name),
      });
      if (res.type === "results") {
        // Belt-and-braces dedupe on top of the exclusion list the backend already sends.
        const seen = new Set(recs.games.map((g) => g.rawgId));
        const fresh = res.games.filter((g) => !seen.has(g.rawgId));
        if (fresh.length === 0) setExhausted(true);
        else setRecs({ reasoning: recs.reasoning, games: [...recs.games, ...fresh] });
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setLoadingMore(false);
    }
  }

  function refineWithShelby() {
    if (!recs) return;
    navigate("/discover/chat", {
      state: { seed: { reasoning: recs.reasoning, games: recs.games } },
    });
  }

  if (loading) {
    return (
      <div className="mt-12 flex items-center gap-2.5 text-sm text-text-lo">
        <DotBounce />
        Finding games based on your activity...
      </div>
    );
  }

  if (error && !recs) {
    return <p className="mt-8 text-sm text-danger">{error}</p>;
  }

  if (!recs || recs.games.length === 0) {
    return (
      <div className="mt-12">
        <EmptyState icon={Compass} title="Nothing to go on yet">
          Add and play a few games — recommendations here are built from what you actually
          spend time in, not just what you save.
        </EmptyState>
      </div>
    );
  }

  return (
    <section className="mt-10 animate-fade-up">
      <p className="shelf-label">For you · based on your activity</p>
      <p className="mt-2.5 max-w-2xl text-[13.5px] leading-relaxed text-text-hi/85">
        {recs.reasoning}
      </p>
      <div className="mt-4 grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4">
        {recs.games.map((game) => (
          <GameCard
            key={game.rawgId}
            game={game}
            footer={
              <AddToLibraryButton game={game} added={addedIds.has(game.rawgId)} onAdd={onAdd} />
            }
          />
        ))}
      </div>

      <div className="mt-5 flex items-center justify-center gap-2.5">
        {!exhausted && (
          <button
            onClick={loadMore}
            disabled={loadingMore}
            className="flex items-center gap-2 rounded-xl border border-border px-4 py-2 text-[12.5px] font-medium text-text-lo transition-all duration-150 hover:border-border-strong hover:text-text-hi enabled:active:scale-[0.98] disabled:opacity-50"
          >
            {loadingMore && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {loadingMore ? "Finding more..." : "Load more"}
          </button>
        )}
        <button
          onClick={refineWithShelby}
          className="flex items-center gap-1.5 rounded-xl border border-border-strong px-4 py-2 text-[12.5px] font-semibold text-text-hi transition-all duration-150 hover:border-accent/50 hover:text-accent-hover active:scale-[0.98]"
        >
          <Sparkles className="h-3.5 w-3.5 text-accent" />
          Refine with Shelby
        </button>
      </div>
      {error && <p className="mt-3 text-center text-sm text-danger">{error}</p>}
    </section>
  );
}
