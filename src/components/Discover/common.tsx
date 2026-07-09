import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check } from "lucide-react";
import type { RawgGameResult } from "../shared/GameCard";
import type { RecommendedGame } from "../../store/useAppStore";

// Response shape shared by `chat_recommend` and `get_dashboard_recommendations`.
export type ChatRecommendResponse =
  | {
      type: "clarify";
      question: string;
      options: string[] | null;
      multiSelect: boolean;
      candidateCount: number | null;
    }
  | { type: "results"; reasoning: string; games: RecommendedGame[] };

export function useAddToLibrary() {
  const [addedIds, setAddedIds] = useState<Set<number>>(new Set());
  const [error, setError] = useState<string | null>(null);

  async function addToLibrary(game: RawgGameResult) {
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

  return { addedIds, addToLibrary, error };
}

export function AddToLibraryButton({
  game,
  added,
  onAdd,
}: {
  game: RawgGameResult;
  added: boolean;
  onAdd: (game: RawgGameResult) => void;
}) {
  if (added) {
    return (
      <span className="flex w-full items-center justify-center gap-1.5 rounded-lg border border-success/25 bg-success/10 px-2 py-1.5 text-xs font-semibold text-success">
        <Check className="h-3.5 w-3.5" /> Added
      </span>
    );
  }
  return (
    <button
      onClick={() => onAdd(game)}
      className="w-full rounded-lg border border-border-strong/60 bg-surface-alt/70 px-2 py-1.5 text-xs font-semibold text-text-hi transition-all duration-150 hover:border-text-hi hover:bg-text-hi hover:text-bg active:scale-[0.98]"
    >
      Add to Library
    </button>
  );
}
