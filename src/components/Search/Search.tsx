import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface RawgGameResult {
  rawgId: number;
  name: string;
  coverUrl: string | null;
  genre: string | null;
  platform: string | null;
}

export function Search() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<RawgGameResult[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [addedIds, setAddedIds] = useState<Set<number>>(new Set());

  async function runSearch(e: React.FormEvent) {
    e.preventDefault();
    if (!query.trim()) return;
    setLoading(true);
    setError(null);
    try {
      const found = await invoke<RawgGameResult[]>("search_rawg", { query });
      setResults(found);
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
      <h1 className="text-2xl font-semibold">Search</h1>
      <p className="mt-2 text-neutral-400">Search RAWG for games to add to your backlog.</p>

      <form onSubmit={runSearch} className="mt-4 flex gap-2">
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search for a game..."
          className="flex-1 rounded bg-neutral-800 px-3 py-2 text-neutral-100 outline-none focus:ring-1 focus:ring-neutral-500"
        />
        <button
          type="submit"
          disabled={loading}
          className="rounded bg-neutral-700 px-4 py-2 font-medium hover:bg-neutral-600 disabled:opacity-50"
        >
          {loading ? "Searching..." : "Search"}
        </button>
      </form>

      {error && <p className="mt-4 text-red-400">{error}</p>}

      <div className="mt-6 grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4">
        {results.map((game) => (
          <div key={game.rawgId} className="overflow-hidden rounded bg-neutral-800">
            {game.coverUrl && (
              <img src={game.coverUrl} alt={game.name} className="h-32 w-full object-cover" />
            )}
            <div className="p-2">
              <p className="truncate font-medium">{game.name}</p>
              {game.genre && <p className="truncate text-xs text-neutral-400">{game.genre}</p>}
              <button
                onClick={() => addToBacklog(game)}
                disabled={addedIds.has(game.rawgId)}
                className="mt-2 w-full rounded bg-neutral-700 px-2 py-1 text-sm hover:bg-neutral-600 disabled:opacity-50"
              >
                {addedIds.has(game.rawgId) ? "Added" : "Add to Backlog"}
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
