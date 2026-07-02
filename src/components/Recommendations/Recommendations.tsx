import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Send, Sparkles, MessageCircle, Check } from "lucide-react";
import { GameCard, type RawgGameResult } from "../shared/GameCard";

type Tab = "for-you" | "chat";

const TABS: { value: Tab; label: string; icon: typeof Sparkles }[] = [
  { value: "for-you", label: "For You", icon: Sparkles },
  { value: "chat", label: "Chat", icon: MessageCircle },
];

interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

type ChatRecommendResponse =
  | { type: "clarify"; question: string; options: string[] | null; multiSelect: boolean }
  | { type: "results"; reasoning: string; games: RawgGameResult[] };

interface Turn {
  user: string;
  assistantText?: string;
  assistantOptions?: string[];
  assistantMultiSelect?: boolean;
  results?: { reasoning: string; games: RawgGameResult[] };
}

function AddToBacklogButton({
  game,
  added,
  onAdd,
}: {
  game: RawgGameResult;
  added: boolean;
  onAdd: (game: RawgGameResult) => void;
}) {
  return (
    <button
      onClick={() => onAdd(game)}
      disabled={added}
      className="w-full rounded bg-neutral-700 px-2 py-1 text-sm hover:bg-neutral-600 disabled:opacity-50"
    >
      {added ? "Added" : "Add to Backlog"}
    </button>
  );
}

function useAddToBacklog() {
  const [addedIds, setAddedIds] = useState<Set<number>>(new Set());
  const [error, setError] = useState<string | null>(null);

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

  return { addedIds, addToBacklog, error };
}

function ForYouTab() {
  const [recs, setRecs] = useState<{ reasoning: string; games: RawgGameResult[] } | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const { addedIds, addToBacklog, error: addError } = useAddToBacklog();

  useEffect(() => {
    invoke<ChatRecommendResponse>("get_dashboard_recommendations")
      .then((res) => {
        if (res.type === "results") setRecs({ reasoning: res.reasoning, games: res.games });
      })
      .catch((err) => setError(String(err)))
      .finally(() => setLoading(false));
  }, []);

  if (loading) {
    return <p className="mt-6 text-sm text-neutral-400">Finding games based on your activity...</p>;
  }

  if (error || addError) {
    return <p className="mt-6 text-sm text-red-400">{error ?? addError}</p>;
  }

  if (!recs || recs.games.length === 0) {
    return (
      <p className="mt-6 text-sm text-neutral-400">
        Add and play a few games to get personalized recommendations here.
      </p>
    );
  }

  return (
    <div className="mt-6">
      <p className="font-medium text-neutral-200">Here are some games based on your activity:</p>
      <p className="mt-1 text-sm text-neutral-400">{recs.reasoning}</p>
      <div className="mt-3 grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4">
        {recs.games.map((game) => (
          <GameCard
            key={game.rawgId}
            game={game}
            footer={
              <AddToBacklogButton game={game} added={addedIds.has(game.rawgId)} onAdd={addToBacklog} />
            }
          />
        ))}
      </div>
    </div>
  );
}

function ThinkingBubble() {
  return (
    <div className="flex w-fit items-center gap-1 rounded-lg bg-neutral-800 px-3 py-2.5">
      <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-neutral-500 [animation-delay:-0.3s]" />
      <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-neutral-500 [animation-delay:-0.15s]" />
      <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-neutral-500" />
    </div>
  );
}

function ChatTab() {
  const [input, setInput] = useState("");
  const [turns, setTurns] = useState<Turn[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { addedIds, addToBacklog, error: addError } = useAddToBacklog();

  // The backend needs to know whether this message answers the clarifying question it just
  // asked (search now) or starts a new ask (clarify first) — we know this directly from what's
  // currently on screen, so we tell it explicitly rather than making the worker guess. The
  // conversation history itself is never truncated, so later asks still have the full prior
  // context to draw on if the user wants to keep exploring/refining.
  const [awaitingAnswer, setAwaitingAnswer] = useState(false);
  const [selectedOptions, setSelectedOptions] = useState<Set<string>>(new Set());

  function toggleOption(option: string) {
    setSelectedOptions((prev) => {
      const next = new Set(prev);
      if (next.has(option)) next.delete(option);
      else next.add(option);
      return next;
    });
  }

  function historyForBackend(): ChatMessage[] {
    const history: ChatMessage[] = [];
    for (const turn of turns) {
      history.push({ role: "user", content: turn.user });
      if (turn.assistantText) {
        history.push({ role: "assistant", content: turn.assistantText });
      } else if (turn.results) {
        history.push({ role: "assistant", content: turn.results.reasoning });
      }
    }
    return history;
  }

  async function send(message: string) {
    const trimmed = message.trim();
    if (!trimmed || loading) return;
    setInput("");
    setSelectedOptions(new Set());
    setError(null);
    setLoading(true);
    try {
      const response = await invoke<ChatRecommendResponse>("chat_recommend", {
        message: trimmed,
        history: historyForBackend(),
        awaitingAnswer,
      });
      if (response.type === "clarify") {
        setTurns((prev) => [
          ...prev,
          {
            user: trimmed,
            assistantText: response.question,
            assistantOptions: response.options ?? undefined,
            assistantMultiSelect: response.multiSelect,
          },
        ]);
        setAwaitingAnswer(true);
      } else {
        setTurns((prev) => [
          ...prev,
          { user: trimmed, results: { reasoning: response.reasoning, games: response.games } },
        ]);
        setAwaitingAnswer(false);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    send(input);
  }

  return (
    <div className="mt-6 flex h-full flex-col">
      <div className="flex-1 space-y-6 overflow-y-auto pb-4">
        {turns.length === 0 && (
          <div className="flex items-center gap-2 rounded border border-neutral-800 bg-neutral-900 p-4 text-neutral-400">
            <MessageCircle className="h-4 w-4 text-emerald-400" />
            Try something like "a relaxing game like Stardew Valley" or "give me a new game" — it'll
            ask a quick follow-up if it needs one.
          </div>
        )}

        {turns.map((turn, i) => {
          const isLastTurn = i === turns.length - 1;
          return (
            <div key={i} className="space-y-3">
              <div className="ml-auto max-w-md rounded-lg bg-emerald-900/40 px-3 py-2 text-right text-sm text-emerald-100">
                {turn.user}
              </div>

              {turn.assistantText && (
                <div className="max-w-md space-y-2">
                  <div className="w-fit rounded-lg bg-neutral-800 px-3 py-2 text-sm text-neutral-200">
                    {turn.assistantText}
                  </div>
                  {isLastTurn && turn.assistantOptions && turn.assistantOptions.length > 0 && (
                    <>
                      <div className="flex flex-wrap gap-2">
                        {turn.assistantOptions.map((option) => {
                          const selected = turn.assistantMultiSelect && selectedOptions.has(option);
                          return (
                            <button
                              key={option}
                              onClick={() =>
                                turn.assistantMultiSelect ? toggleOption(option) : send(option)
                              }
                              disabled={loading}
                              className={`flex items-center gap-1.5 rounded-full border px-3 py-1 text-sm disabled:opacity-50 ${
                                selected
                                  ? "border-emerald-700 bg-emerald-900/40 text-emerald-200"
                                  : "border-neutral-700 bg-neutral-900 text-neutral-200 hover:bg-neutral-800"
                              }`}
                            >
                              {selected && <Check className="h-3.5 w-3.5" />}
                              {option}
                            </button>
                          );
                        })}
                      </div>
                      {turn.assistantMultiSelect && (
                        <button
                          onClick={() => send(Array.from(selectedOptions).join(", "))}
                          disabled={loading || selectedOptions.size === 0}
                          className="rounded bg-emerald-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-emerald-600 disabled:opacity-40"
                        >
                          Continue
                        </button>
                      )}
                    </>
                  )}
                </div>
              )}

              {turn.results && (
                <div>
                  <p className="font-medium text-neutral-200">Here are some games based on your answers:</p>
                  <p className="mt-1 text-sm text-neutral-400">{turn.results.reasoning}</p>
                  <div className="mt-3 grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4">
                    {turn.results.games.map((game) => (
                      <GameCard
                        key={game.rawgId}
                        game={game}
                        footer={
                          <AddToBacklogButton
                            game={game}
                            added={addedIds.has(game.rawgId)}
                            onAdd={addToBacklog}
                          />
                        }
                      />
                    ))}
                  </div>
                  {isLastTurn && (
                    <p className="mt-4 text-sm text-neutral-500">
                      Want to keep exploring? Send another message to refine or ask for something
                      different.
                    </p>
                  )}
                </div>
              )}
            </div>
          );
        })}

        {loading && <ThinkingBubble />}
      </div>

      {(error || addError) && <p className="mb-2 text-sm text-red-400">{error ?? addError}</p>}

      <form onSubmit={handleSubmit} className="flex gap-2 border-t border-neutral-800 pt-3">
        <input
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          disabled={loading}
          placeholder={loading ? "Waiting for a response..." : "Describe a game or a mood..."}
          className="flex-1 rounded bg-neutral-800 px-3 py-2 text-neutral-100 outline-none focus:ring-1 focus:ring-neutral-500 disabled:opacity-50"
        />
        <button
          type="submit"
          disabled={loading}
          className="flex items-center gap-1 rounded bg-neutral-700 px-4 py-2 font-medium hover:bg-neutral-600 disabled:opacity-50"
        >
          <Send className="h-4 w-4" />
          Send
        </button>
      </form>
    </div>
  );
}

export function Recommendations() {
  const [tab, setTab] = useState<Tab>("for-you");

  return (
    <div className="flex h-full flex-col">
      <h1 className="text-2xl font-semibold">Recommendations</h1>
      <p className="mt-2 text-neutral-400">
        Games picked from your activity, or describe what you're in the mood for and chat it out.
      </p>

      <div className="mt-4 flex gap-1 rounded-lg bg-neutral-900 p-1">
        {TABS.map(({ value, label, icon: Icon }) => (
          <button
            key={value}
            onClick={() => setTab(value)}
            className={`flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm transition-colors ${
              tab === value ? "bg-indigo-600 text-white" : "text-neutral-400 hover:text-neutral-200"
            }`}
          >
            <Icon className="h-3.5 w-3.5" />
            {label}
          </button>
        ))}
      </div>

      {tab === "for-you" ? <ForYouTab /> : <ChatTab />}
    </div>
  );
}
