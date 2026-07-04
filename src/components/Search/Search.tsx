import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Search as SearchIcon, Loader2 } from "lucide-react";
import { GameCard, type RawgGameResult } from "../shared/GameCard";

export function Search() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<RawgGameResult[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [addedIds, setAddedIds] = useState<Set<number>>(new Set());
  const [hasSearched, setHasSearched] = useState(false);

  async function runSearch(e: React.FormEvent) {
    e.preventDefault();
    if (!query.trim()) return;
    setLoading(true);
    setError(null);
    try {
      const found = await invoke<RawgGameResult[]>("search_rawg", { query });
      setResults(found);
      setHasSearched(true);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  async function addToBacklog(game: RawgGameResult) {
    try {
      await invoke("add_game", {
        rawgId: game.rawgId,
        name: game.name,
        coverUrl: game.coverUrl,
        genre: game.genre,
        platform: game.platform,
      });
      setAddedIds((prev) => new Set(prev).add(game.rawgId));
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <div>
      <h1 className="page-title text-[26px]">Search</h1>
      <p className="mt-1.5 text-[13.5px] text-text-lo">
        Find games on RAWG and add them straight to your backlog.
      </p>

      <form onSubmit={runSearch} className="mt-6 flex gap-2">
        <div className="relative flex-1">
          <SearchIcon className="pointer-events-none absolute left-3.5 top-1/2 h-4 w-4 -translate-y-1/2 text-text-lo" />
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search for a game..."
            className="w-full rounded-xl border border-border bg-surface py-2.5 pl-10 pr-3 text-sm text-text-hi outline-none transition-colors placeholder:text-text-lo/70 focus:border-accent/50"
          />
        </div>
        <button
          type="submit"
          disabled={loading}
          className="flex items-center gap-2 rounded-xl bg-accent px-5 py-2.5 text-sm font-semibold text-bg transition-colors hover:bg-accent-hover disabled:opacity-50"
        >
          {loading && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          {loading ? "Searching..." : "Search"}
        </button>
      </form>

      {error && <p className="mt-4 text-sm text-danger">{error}</p>}

      {results.length > 0 && (
        <div className="mt-7 grid animate-fade-up grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4">
          {results.map((game) => (
            <GameCard
              key={game.rawgId}
              game={game}
              footer={
                <button
                  onClick={() => addToBacklog(game)}
                  disabled={addedIds.has(game.rawgId)}
                  className="w-full rounded-lg bg-surface-alt px-2 py-1.5 text-xs font-semibold text-text-hi transition-colors hover:bg-accent hover:text-bg disabled:opacity-50 disabled:hover:bg-surface-alt disabled:hover:text-text-hi"
                >
                  {addedIds.has(game.rawgId) ? "Added" : "Add to Backlog"}
                </button>
              }
            />
          ))}
        </div>
      )}

      {!loading && !error && hasSearched && results.length === 0 && (
        <div className="mt-8 rounded-2xl border border-dashed border-border py-14 text-center">
          <SearchIcon className="mx-auto h-8 w-8 text-text-lo" />
          <p className="mt-3 text-sm font-semibold text-text-hi">No games found</p>
          <p className="mt-1 text-[13px] text-text-lo">
            Try a different title or check the spelling.
          </p>
        </div>
      )}

      {!hasSearched && (
        <div className="mt-8 rounded-2xl border border-dashed border-border py-14 text-center">
          <SearchIcon className="mx-auto h-8 w-8 text-text-lo" />
          <p className="mt-3 text-sm font-semibold text-text-hi">Search for a game</p>
          <p className="mt-1 text-[13px] text-text-lo">
            Look up a title to see cover art, genres, and add it to your backlog.
          </p>
        </div>
      )}
    </div>
  );
}
