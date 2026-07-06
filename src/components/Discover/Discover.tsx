import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useNavigate } from "react-router-dom";
import { Search as SearchIcon, Loader2, SearchX, Sparkles, Compass } from "lucide-react";
import { GameCard, type RawgGameResult } from "../shared/GameCard";
import { EmptyState } from "../shared/EmptyState";
import { AddToLibraryButton, useAddToLibrary, type ChatRecommendResponse } from "./common";

/**
 * The merged find-a-game surface (task 20): one find box up top with two explicit
 * actions — Search (RAWG title lookup) and Ask Shelby (drops into the chat flow at
 * /discover/chat, carrying any typed text as the first message). "For You"
 * recommendations are the page's idle content; a search swaps them for the results
 * grid until cleared.
 */
export function Discover() {
  const navigate = useNavigate();
  const [query, setQuery] = useState("");
  // null = no active search (For You shows); [] = a search that found nothing.
  const [results, setResults] = useState<RawgGameResult[] | null>(null);
  const [searchedQuery, setSearchedQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { addedIds, addToLibrary, error: addError } = useAddToLibrary();

  async function runSearch(e: React.FormEvent) {
    e.preventDefault();
    const trimmed = query.trim();
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
            className="flex shrink-0 items-center gap-1.5 rounded-xl border border-border-strong px-3.5 py-2 text-[13px] font-semibold text-text-hi transition-colors hover:border-accent/50 hover:text-accent-hover"
          >
            <Sparkles className="h-3.5 w-3.5 text-accent" />
            Ask Shelby
          </button>
          <button
            type="submit"
            disabled={loading}
            className="flex shrink-0 items-center gap-2 rounded-xl bg-text-hi px-[18px] py-2 text-[13px] font-semibold text-bg transition-opacity hover:opacity-85 disabled:opacity-50"
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
  const [recs, setRecs] = useState<{ reasoning: string; games: RawgGameResult[] } | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<ChatRecommendResponse>("get_dashboard_recommendations")
      .then((res) => {
        if (res.type === "results") setRecs({ reasoning: res.reasoning, games: res.games });
      })
      .catch((err) => setError(String(err)))
      .finally(() => setLoading(false));
  }, []);

  if (loading) {
    return (
      <div className="mt-12 flex items-center gap-2.5 text-sm text-text-lo">
        <span className="flex gap-1">
          <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-accent [animation-delay:-0.3s]" />
          <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-accent [animation-delay:-0.15s]" />
          <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-accent" />
        </span>
        Finding games based on your activity...
      </div>
    );
  }

  if (error) {
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
    </section>
  );
}
