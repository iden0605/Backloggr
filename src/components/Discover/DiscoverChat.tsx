import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useLocation, useNavigate } from "react-router-dom";
import { Send, Sparkles, MessageCircle, Check, ArrowLeft } from "lucide-react";
import { GameCard } from "../shared/GameCard";
import { useAppStore } from "../../store/useAppStore";
import { AddToLibraryButton, useAddToLibrary, type ChatRecommendResponse } from "./common";

interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

function ThinkingBubble() {
  return (
    <div className="flex items-center gap-2.5 px-1 py-1 text-text-lo">
      <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-accent/10 text-accent">
        <Sparkles className="h-3 w-3" />
      </div>
      <span className="flex items-center gap-1">
        <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-text-lo/60 [animation-delay:-0.3s]" />
        <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-text-lo/60 [animation-delay:-0.15s]" />
        <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-text-lo/60" />
      </span>
    </div>
  );
}

const TEXTAREA_MAX_HEIGHT = 160;

/**
 * The AI half of the Discover surface (/discover/chat) — a full-page chat takeover
 * reached via the find box's "Ask AI" action, which may carry the typed text along
 * as the conversation's next message (router state `ask`). Conversation state lives
 * in the Zustand store, so leaving and coming back resumes where you were.
 */
export function DiscoverChat() {
  const navigate = useNavigate();
  const location = useLocation();
  const [input, setInput] = useState("");
  const turns = useAppStore((s) => s.chatTurns);
  const setTurns = useAppStore((s) => s.setChatTurns);
  const questionsAsked = useAppStore((s) => s.chatQuestionsAsked);
  const setQuestionsAsked = useAppStore((s) => s.setChatQuestionsAsked);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { addedIds, addToLibrary, error: addError } = useAddToLibrary();
  const [selectedOptions, setSelectedOptions] = useState<Set<string>>(new Set());
  const bottomRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const autoSentRef = useRef(false);

  // Auto-scroll to the latest message on every new turn or reply, and again whenever the
  // page remounts (e.g. navigating back to the chat) so returning always lands at the bottom.
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [turns, loading]);

  // "Ask AI" from the find box carries the typed text as router state — send it as the
  // opening message once, then clear the state so a remount/back can't resend it.
  useEffect(() => {
    const ask = (location.state as { ask?: string } | null)?.ask;
    if (ask && !autoSentRef.current) {
      autoSentRef.current = true;
      navigate(location.pathname, { replace: true, state: null });
      send(ask);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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
        // Include the actual titles shown, not just the reasoning line — otherwise a
        // follow-up like "something more modern than these" has no referent and the
        // model happily regenerates the same set.
        const shown = turn.results.games.map((g) => g.name).join(", ");
        history.push({
          role: "assistant",
          content: shown
            ? `${turn.results.reasoning} (I recommended: ${shown})`
            : turn.results.reasoning,
        });
      }
    }
    return history;
  }

  async function send(message: string) {
    const trimmed = message.trim();
    if (!trimmed || loading) return;
    setInput("");
    if (textareaRef.current) textareaRef.current.style.height = "auto";
    setSelectedOptions(new Set());
    setError(null);
    setLoading(true);
    try {
      const response = await invoke<ChatRecommendResponse>("chat_recommend", {
        message: trimmed,
        history: historyForBackend(),
        questionsAsked,
      });
      if (response.type === "clarify") {
        setTurns((prev) => [
          ...prev,
          {
            user: trimmed,
            assistantText: response.question,
            assistantOptions: response.options ?? undefined,
            assistantMultiSelect: response.multiSelect,
            candidateCount: response.candidateCount,
          },
        ]);
        setQuestionsAsked((n) => n + 1);
      } else {
        setTurns((prev) => [
          ...prev,
          { user: trimmed, results: { reasoning: response.reasoning, games: response.games } },
        ]);
        setQuestionsAsked(0);
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

  function handleKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send(input);
    }
  }

  function handleInput(e: React.ChangeEvent<HTMLTextAreaElement>) {
    setInput(e.target.value);
    const el = e.target;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, TEXTAREA_MAX_HEIGHT)}px`;
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 items-center gap-4">
        <button
          onClick={() => navigate("/discover")}
          className="flex items-center gap-1.5 rounded-lg border border-border px-3 py-1.5 text-[12.5px] font-medium text-text-lo transition-colors hover:border-border-strong hover:text-text-hi"
        >
          <ArrowLeft className="h-3.5 w-3.5" />
          Back to browse
        </button>
        <h1 className="page-title text-[20px]">Ask AI</h1>
      </div>

      <div className="mt-3 min-h-0 flex-1 overflow-y-auto">
        {turns.length === 0 && !loading && (
          <div className="flex h-full min-h-[280px] animate-fade-up flex-col items-center justify-center gap-3 text-center">
            <div className="flex h-11 w-11 items-center justify-center rounded-2xl bg-accent/10 text-accent">
              <MessageCircle className="h-5 w-5" />
            </div>
            <p className="max-w-sm text-[13.5px] leading-relaxed text-text-lo">
              Tell me what you're in the mood for — a genre, a game you loved, a vibe. I'll ask a
              question or two to narrow it down, then pull together a shortlist.
            </p>
          </div>
        )}

        <div className="mx-auto max-w-2xl space-y-7 px-1 pb-8 pt-2">
          {turns.map((turn, i) => {
            const isLastTurn = i === turns.length - 1;
            return (
              <div key={i} className="animate-fade-up space-y-4">
                <div className="flex justify-end">
                  <div className="max-w-[80%] rounded-2xl rounded-br-md border border-border-strong/50 bg-surface-alt px-4 py-2.5 text-[13.5px] leading-relaxed text-text-hi">
                    {turn.user}
                  </div>
                </div>

                {turn.assistantText && (
                  <div className="flex gap-3">
                    <div className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-accent/10 text-accent">
                      <Sparkles className="h-3 w-3" />
                    </div>
                    <div className="min-w-0 flex-1 space-y-3">
                      <p className="text-[13.5px] leading-relaxed text-text-hi">{turn.assistantText}</p>

                      {typeof turn.candidateCount === "number" && turn.candidateCount > 8 && (
                        <p className="font-mono text-[10.5px] uppercase tracking-wide text-text-lo/70">
                          {turn.candidateCount} games fit so far · narrowing down
                        </p>
                      )}

                      {isLastTurn && turn.assistantOptions && turn.assistantOptions.length > 0 && (
                        <div className="space-y-2.5">
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
                                  className={`flex items-center gap-1.5 rounded-full border px-3.5 py-1.5 text-[12.5px] font-medium transition-all duration-150 disabled:opacity-50 ${
                                    selected
                                      ? "border-accent/50 bg-accent/15 text-accent"
                                      : "border-border bg-surface text-text-hi hover:border-accent/30 hover:bg-surface-alt"
                                  }`}
                                >
                                  {selected && <Check className="h-3 w-3" />}
                                  {option}
                                </button>
                              );
                            })}
                          </div>
                          {turn.assistantMultiSelect && (
                            <button
                              onClick={() => send(Array.from(selectedOptions).join(", "))}
                              disabled={loading || selectedOptions.size === 0}
                              className="rounded-full bg-text-hi px-4 py-1.5 text-[12.5px] font-semibold text-bg transition-opacity hover:opacity-85 disabled:cursor-not-allowed disabled:opacity-30"
                            >
                              Continue
                            </button>
                          )}
                        </div>
                      )}
                    </div>
                  </div>
                )}

                {turn.results && (
                  <div className="flex gap-3">
                    <div className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-accent/10 text-accent">
                      <Sparkles className="h-3 w-3" />
                    </div>
                    <div className="min-w-0 flex-1">
                      <p className="text-[13.5px] leading-relaxed text-text-hi">{turn.results.reasoning}</p>
                      <div className="mt-4 grid grid-cols-2 gap-3.5 sm:grid-cols-3">
                        {turn.results.games.map((game) => (
                          <GameCard
                            key={game.rawgId}
                            game={game}
                            note={game.reason}
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
                      {isLastTurn && (
                        <p className="mt-4 text-[12.5px] text-text-lo/80">
                          Not quite it? Send another message and I'll adjust — or ask for something
                          else entirely.
                        </p>
                      )}
                    </div>
                  </div>
                )}
              </div>
            );
          })}

          {loading && <ThinkingBubble />}
          <div ref={bottomRef} />
        </div>
      </div>

      {(error || addError) && (
        <p className="mx-auto mt-2 max-w-2xl text-sm text-danger">{error ?? addError}</p>
      )}

      <form onSubmit={handleSubmit} className="mx-auto w-full max-w-2xl shrink-0 pb-1 pt-3">
        <div className="flex items-end gap-2 rounded-2xl border border-border bg-surface py-1.5 pl-4 pr-1.5 shadow-[0_8px_24px_-12px_rgba(0,0,0,0.5)] transition-colors focus-within:border-accent/40">
          <textarea
            ref={textareaRef}
            value={input}
            onChange={handleInput}
            onKeyDown={handleKeyDown}
            rows={1}
            placeholder="Describe a game or a mood..."
            style={{ maxHeight: TEXTAREA_MAX_HEIGHT }}
            className="block max-h-40 flex-1 resize-none overflow-y-auto bg-transparent py-1 leading-6 text-[13.5px] text-text-hi outline-none placeholder:text-text-lo/70"
          />
          <button
            type="submit"
            disabled={loading || !input.trim()}
            className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-text-hi text-bg transition-opacity hover:opacity-85 disabled:cursor-not-allowed disabled:bg-surface-alt disabled:text-text-lo disabled:opacity-100"
          >
            <Send className="h-3.5 w-3.5" />
          </button>
        </div>
        <p className="mt-2 text-center font-mono text-[10px] uppercase tracking-wide text-text-lo/50">
          Enter to send · Shift + Enter for a new line
        </p>
      </form>
    </div>
  );
}
