import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useLocation, useNavigate } from "react-router-dom";
import {
  Send,
  Sparkles,
  Gamepad2,
  Check,
  ArrowLeft,
  Plus,
  X,
  PanelLeftClose,
  PanelLeftOpen,
} from "lucide-react";
import { GameCard } from "../shared/GameCard";
import { useAppStore, type ChatTurn, type RecommendedGame } from "../../store/useAppStore";
import { formatRelative, type LibraryGame } from "../Library/Library";
import { AddToLibraryButton, useAddToLibrary, type ChatRecommendResponse } from "./common";

interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

// Row shapes of the chat_conversations commands (list_chats / get_chat).
interface ChatSummary {
  id: number;
  title: string;
  updatedAt: string;
}

interface ChatConversation {
  id: number;
  title: string;
  turnsJson: string;
  questionsAsked: number;
}

// Router state the page can arrive with: `ask` carries the find box's typed text as an
// opening message; `seed` carries the For You set from "Refine with Shelby" (task 21).
interface ChatRouterState {
  ask?: string;
  seed?: { reasoning: string; games: RecommendedGame[] };
}

// Laid out identically to a turn's assistant row (same gap/avatar geometry) so the
// reply appears to take the thinking indicator's place. The entrance delay keeps a
// fast response from flashing it for a single frame.
function ThinkingBubble() {
  return (
    <div className="flex animate-fade-in gap-3 text-text-lo [animation-delay:150ms]">
      <div className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-accent/10 text-accent">
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

const USER_BUBBLE_CLASS =
  "max-w-[80%] rounded-2xl rounded-br-md border border-border-strong/50 bg-surface-alt px-4 py-2.5 text-[13.5px] leading-relaxed text-text-hi";

const TEXTAREA_MAX_HEIGHT = 160;

// Empty-state quick prompts — one tap instead of a blank composer. Labels stay short
// (they're chips); the message sent is a natural full ask so the narrowing loop has
// something real to work with.
const MOOD_PROMPTS: { label: string; message: string }[] = [
  { label: "RPG", message: "I'm in the mood for a great RPG" },
  { label: "Cozy", message: "Something cozy and low-stress" },
  { label: "Horror", message: "A horror game that will actually scare me" },
  { label: "Multiplayer", message: "A multiplayer game to play with friends" },
  { label: "Open world", message: "A big open-world game to get lost in" },
  { label: "Roguelike", message: "A roguelike I can replay forever" },
];

/**
 * Shelby — the AI half of the Discover surface (/discover/chat), a full-page chat
 * takeover reached via the find box's "Ask Shelby" action, which may carry the typed
 * text along as the conversation's next message (router state `ask`). Conversation
 * state lives in the Zustand store, so leaving and coming back resumes where you were.
 */
export function DiscoverChat() {
  const navigate = useNavigate();
  const location = useLocation();
  const [input, setInput] = useState("");
  const turns = useAppStore((s) => s.chatTurns);
  const setTurns = useAppStore((s) => s.setChatTurns);
  const questionsAsked = useAppStore((s) => s.chatQuestionsAsked);
  const setQuestionsAsked = useAppStore((s) => s.setChatQuestionsAsked);
  const chatId = useAppStore((s) => s.chatId);
  const setChatId = useAppStore((s) => s.setChatId);
  const chatHydrated = useAppStore((s) => s.chatHydrated);
  const setChatHydrated = useAppStore((s) => s.setChatHydrated);
  const [loading, setLoading] = useState(false);
  // The in-flight user message, shown optimistically the moment it's sent — the turn
  // itself only lands in the store once the AI replies.
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { addedIds, addToLibrary, error: addError } = useAddToLibrary();
  const [selectedOptions, setSelectedOptions] = useState<Set<string>>(new Set());
  // Sidebar conversation list — fetched on mount, refreshed after every save/delete.
  const [chats, setChats] = useState<ChatSummary[] | null>(null);
  // Collapsed/expanded state sticks across visits and app restarts.
  const [sidebarOpen, setSidebarOpen] = useState(
    () => localStorage.getItem("chatSidebarOpen") !== "0"
  );
  const bottomRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const autoSentRef = useRef(false);
  // Turns already in the store when the page mounted (i.e. restored history) render
  // without entrance animations — only turns that arrive live animate in.
  const initialTurnCount = useRef(turns.length);

  // Auto-scroll to the latest message on every new turn or reply, and again whenever the
  // page remounts (e.g. navigating back to the chat) so returning always lands at the bottom.
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [turns, loading, pending]);

  // "Ask Shelby" from the find box carries the typed text as router state — send it as the
  // opening message once, then clear the state so a remount/back can't resend it. "Refine
  // with Shelby" from the For You grid carries the recommendation set instead: a fresh
  // conversation opens with that set as its first turn, ready to be refined in place.
  useEffect(() => {
    const state = location.state as ChatRouterState | null;
    if (autoSentRef.current) return;
    if (state?.ask) {
      autoSentRef.current = true;
      navigate(location.pathname, { replace: true, state: null });
      send(state.ask);
    } else if (state?.seed) {
      autoSentRef.current = true;
      navigate(location.pathname, { replace: true, state: null });
      seedFromForYou(state.seed);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Restore the most recent conversation once per app run (Zustand is memory-only, so a
  // restart lands here with empty turns). Skipped when the find box carried an "ask" or
  // the For You grid carried a seed — those start fresh instead of splicing into last
  // session's conversation.
  useEffect(() => {
    if (chatHydrated) return;
    setChatHydrated(true);
    const state = location.state as ChatRouterState | null;
    if (turns.length > 0 || state?.ask || state?.seed) return;
    (async () => {
      try {
        const chats = await invoke<ChatSummary[]>("list_chats");
        // Bail if the user already started typing/sending while we fetched.
        if (chats.length === 0 || useAppStore.getState().chatTurns.length > 0) return;
        await loadChat(chats[0].id);
      } catch {
        // Best-effort restore — a fresh chat is a fine fallback.
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Populate the sidebar list on mount.
  useEffect(() => {
    void refreshChats();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The player's top-played games feed the empty state's "More like..." quick prompts —
  // personalized starters beat hardcoded examples. Best-effort; chips just don't show on failure.
  const [lovedGames, setLovedGames] = useState<string[]>([]);
  useEffect(() => {
    invoke<LibraryGame[]>("get_library")
      .then((games) =>
        setLovedGames(
          games
            .filter((g) => g.totalSeconds > 0)
            .sort((a, b) => b.totalSeconds - a.totalSeconds)
            .slice(0, 3)
            .map((g) => g.name)
        )
      )
      .catch(() => {});
  }, []);

  async function refreshChats() {
    try {
      setChats(await invoke<ChatSummary[]>("list_chats"));
    } catch (err) {
      console.warn("failed to list chats:", err);
    }
  }

  function toggleSidebar() {
    setSidebarOpen((open) => {
      localStorage.setItem("chatSidebarOpen", open ? "0" : "1");
      return !open;
    });
  }

  // Best-effort save after each completed exchange; adopts the row id on first save.
  async function persistChat(nextTurns: ChatTurn[], nextQuestionsAsked: number) {
    const title = (nextTurns[0]?.user ?? "New chat").slice(0, 80);
    try {
      const id = await invoke<number>("save_chat", {
        id: chatId,
        title,
        turnsJson: JSON.stringify(nextTurns),
        questionsAsked: nextQuestionsAsked,
      });
      if (id !== chatId) setChatId(id);
      // Title/ordering may have changed (or a new row appeared) — keep the sidebar current.
      void refreshChats();
    } catch (err) {
      console.warn("failed to persist chat:", err);
    }
  }

  async function loadChat(id: number) {
    if (loading) return;
    const chat = await invoke<ChatConversation>("get_chat", { id });
    const loaded = JSON.parse(chat.turnsJson) as ChatTurn[];
    // Loaded turns are history, not live arrivals — render them without entrance animations.
    initialTurnCount.current = loaded.length;
    setTurns(loaded);
    setQuestionsAsked(chat.questionsAsked);
    setChatId(chat.id);
    setSelectedOptions(new Set());
    setError(null);
  }

  // "Refine with Shelby": a new conversation whose first turn IS the For You set, so
  // follow-ups refine it through the normal history mechanism (the shown titles go back to
  // the model as "(I recommended: ...)"). Not persisted until the user actually replies —
  // an untouched hand-off shouldn't clutter the history sidebar.
  function seedFromForYou(seed: NonNullable<ChatRouterState["seed"]>) {
    initialTurnCount.current = 0;
    setTurns([
      {
        user: "What should I play next?",
        results: { reasoning: seed.reasoning, games: seed.games },
      },
    ]);
    setQuestionsAsked(0);
    setChatId(null);
    setSelectedOptions(new Set());
    setError(null);
    textareaRef.current?.focus();
  }

  function startNewChat() {
    if (loading) return;
    initialTurnCount.current = 0;
    setTurns([]);
    setQuestionsAsked(0);
    setChatId(null);
    setSelectedOptions(new Set());
    setError(null);
    textareaRef.current?.focus();
  }

  async function deleteChat(id: number) {
    try {
      await invoke("delete_chat", { id });
      setChats((prev) => (prev ? prev.filter((c) => c.id !== id) : prev));
      // Deleting the conversation that's on screen resets to a fresh chat.
      if (id === chatId) {
        initialTurnCount.current = 0;
        setTurns([]);
        setQuestionsAsked(0);
        setChatId(null);
        setSelectedOptions(new Set());
      }
    } catch (err) {
      setError(String(err));
    }
  }

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
    setPending(trimmed);
    setLoading(true);
    try {
      const response = await invoke<ChatRecommendResponse>("chat_recommend", {
        message: trimmed,
        history: historyForBackend(),
        questionsAsked,
      });
      let newTurn: ChatTurn;
      let nextQuestionsAsked: number;
      if (response.type === "clarify") {
        newTurn = {
          user: trimmed,
          assistantText: response.question,
          assistantOptions: response.options ?? undefined,
          assistantMultiSelect: response.multiSelect,
          candidateCount: response.candidateCount,
        };
        nextQuestionsAsked = questionsAsked + 1;
      } else {
        newTurn = {
          user: trimmed,
          results: { reasoning: response.reasoning, games: response.games },
        };
        nextQuestionsAsked = 0;
      }
      const nextTurns = [...turns, newTurn];
      setTurns(nextTurns);
      setQuestionsAsked(nextQuestionsAsked);
      void persistChat(nextTurns, nextQuestionsAsked);
    } catch (err) {
      setError(String(err));
      // Put the failed message back in the composer so a retry is one keypress away.
      setInput(trimmed);
    } finally {
      setPending(null);
      setLoading(false);
      textareaRef.current?.focus();
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
    <div className="relative flex h-full min-h-0 flex-col">
      {/* Barely-there warm radial wash so the page isn't a flat near-black field. */}
      <div className="pointer-events-none absolute inset-x-0 -top-24 -z-10 h-80 bg-[radial-gradient(ellipse_70%_100%_at_50%_0%,rgba(185,106,85,0.07),transparent_70%)]" />
      <div className="flex shrink-0 items-center gap-4">
        <button
          onClick={() => navigate("/discover")}
          className="flex items-center gap-1.5 rounded-lg border border-border px-3 py-1.5 text-[12.5px] font-medium text-text-lo transition-colors hover:border-border-strong hover:text-text-hi"
        >
          <ArrowLeft className="h-3.5 w-3.5" />
          Back to browse
        </button>
        <h1 className="page-title text-[20px]">Shelby</h1>
      </div>

      <div className="mt-3 flex min-h-0 flex-1 gap-6">
        <aside
          className={`relative shrink-0 overflow-hidden transition-[width] duration-200 ease-out ${
            sidebarOpen ? "w-56" : "w-10"
          }`}
        >
          {/* Expanded layer — fixed at w-56 so its content clips instead of reflowing
              while the width animates. */}
          <div
            className={`flex h-full w-56 flex-col transition-opacity duration-150 ${
              sidebarOpen ? "opacity-100" : "pointer-events-none opacity-0"
            }`}
          >
            <div className="flex items-stretch gap-2">
              <button
                onClick={startNewChat}
                className="flex flex-1 items-center justify-center gap-1.5 rounded-lg bg-text-hi px-3 py-2 text-[12.5px] font-semibold text-bg transition-opacity hover:opacity-85"
              >
                <Plus className="h-3.5 w-3.5" />
                New chat
              </button>
              <button
                onClick={toggleSidebar}
                title="Hide history"
                className="flex w-9 shrink-0 items-center justify-center rounded-lg border border-border text-text-lo transition-colors hover:border-border-strong hover:text-text-hi"
              >
                <PanelLeftClose className="h-4 w-4" />
              </button>
            </div>

            <p className="shelf-label mt-5 px-1">History</p>
            <div className="-mr-1 mt-2 min-h-0 flex-1 space-y-0.5 overflow-y-auto pr-1">
              {chats === null ? null : chats.length === 0 ? (
                <p className="px-1 py-1.5 text-xs leading-relaxed text-text-lo/80">
                  No past chats yet — conversations save here automatically.
                </p>
              ) : (
                chats.map((chat) => {
                  const active = chat.id === chatId;
                  return (
                    <div
                      key={chat.id}
                      className={`group relative flex items-center gap-1 rounded-lg pr-1 transition-colors ${
                        active ? "bg-surface-alt" : "hover:bg-surface"
                      }`}
                    >
                      {active && (
                        <span className="absolute left-0 top-1/2 h-4 w-0.5 -translate-y-1/2 rounded-full bg-accent" />
                      )}
                      <button
                        onClick={() => loadChat(chat.id).catch((err) => setError(String(err)))}
                        className="flex min-w-0 flex-1 flex-col gap-0.5 px-2.5 py-2 text-left"
                      >
                        <span
                          className={`truncate text-xs font-medium ${
                            active ? "text-text-hi" : "text-text-lo group-hover:text-text-hi"
                          }`}
                        >
                          {chat.title}
                        </span>
                        <span className="font-mono text-[10px] uppercase tracking-wide text-text-lo/60">
                          {formatRelative(chat.updatedAt)}
                        </span>
                      </button>
                      <button
                        onClick={() => deleteChat(chat.id)}
                        title="Delete chat"
                        className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-text-lo opacity-0 transition-all hover:bg-danger/15 hover:text-danger group-hover:opacity-100"
                      >
                        <X className="h-3.5 w-3.5" />
                      </button>
                    </div>
                  );
                })
              )}
            </div>
          </div>

          {/* Collapsed rail — expand + new chat as icon buttons. */}
          <div
            className={`absolute inset-y-0 left-0 flex w-10 flex-col items-center gap-2 transition-opacity duration-150 ${
              sidebarOpen ? "pointer-events-none opacity-0" : "opacity-100"
            }`}
          >
            <button
              onClick={toggleSidebar}
              title="Show history"
              className="flex h-9 w-9 items-center justify-center rounded-lg border border-border text-text-lo transition-colors hover:border-border-strong hover:text-text-hi"
            >
              <PanelLeftOpen className="h-4 w-4" />
            </button>
            <button
              onClick={startNewChat}
              title="New chat"
              className="flex h-9 w-9 items-center justify-center rounded-lg border border-border text-text-lo transition-colors hover:border-border-strong hover:text-text-hi"
            >
              <Plus className="h-4 w-4" />
            </button>
          </div>
        </aside>

        <div className="flex min-h-0 flex-1 flex-col">
          <div className="min-h-0 flex-1 overflow-y-auto">
            {turns.length === 0 && !loading && !pending && (
              <div className="mx-auto flex max-w-xl animate-fade-up flex-col items-center gap-3 pt-10 text-center">
                <div className="flex h-14 w-14 items-center justify-center rounded-full border border-accent/30 bg-accent/10 text-accent shadow-[0_0_28px_-4px_rgba(185,106,85,0.4)]">
                  <Gamepad2 className="h-6 w-6" />
                </div>
                <p className="page-title mt-1 text-[22px]">Looking for your next obsession?</p>
                <p className="max-w-md text-[13.5px] leading-relaxed text-text-lo">
                  Tell me a game you loved, a genre, or just your mood — I'll help you find
                  something you'll actually enjoy.
                </p>

                <div className="mt-5 w-full space-y-5">
                  <div>
                    <p className="shelf-label">In the mood for</p>
                    <div className="mt-2.5 flex flex-wrap justify-center gap-2">
                      {MOOD_PROMPTS.map((prompt, i) => (
                        <button
                          key={prompt.label}
                          onClick={() => send(prompt.message)}
                          style={{ animationDelay: `${100 + i * 40}ms` }}
                          className="animate-fade-up rounded-full border border-border bg-surface px-3.5 py-1.5 text-[12.5px] font-medium text-text-hi transition-all duration-150 hover:border-accent/40 hover:bg-accent/10 hover:text-accent-hover"
                        >
                          {prompt.label}
                        </button>
                      ))}
                    </div>
                  </div>

                  {lovedGames.length > 0 && (
                    <div>
                      <p className="shelf-label">More like your favorites</p>
                      <div className="mt-2.5 flex flex-wrap justify-center gap-2">
                        {lovedGames.map((name, i) => (
                          <button
                            key={name}
                            onClick={() => send(`I loved ${name} — what should I play next?`)}
                            style={{ animationDelay: `${340 + i * 40}ms` }}
                            className="animate-fade-up flex items-center gap-1.5 rounded-full border border-border bg-surface px-3.5 py-1.5 text-[12.5px] font-medium text-text-hi transition-all duration-150 hover:border-accent/40 hover:bg-accent/10 hover:text-accent-hover"
                          >
                            <Sparkles className="h-3 w-3 text-accent" />
                            More like {name}
                          </button>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
              </div>
            )}

            <div className="mx-auto max-w-2xl space-y-7 px-1 pb-8 pt-2">
              {turns.map((turn, i) => {
                const isLastTurn = i === turns.length - 1;
                // Turns restored from the store render static; only live arrivals animate.
                const live = i >= initialTurnCount.current;
                const fadeUp = live ? "animate-fade-up" : "";
                return (
                  <div key={i} className="space-y-4">
                    <div className="flex justify-end">
                      <div className={USER_BUBBLE_CLASS}>{turn.user}</div>
                    </div>

                    {turn.assistantText && (
                      <div className="flex gap-3">
                        <div
                          className={`mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-accent/10 text-accent ${fadeUp}`}
                        >
                          <Sparkles className="h-3 w-3" />
                        </div>
                        <div className="min-w-0 flex-1 space-y-3">
                          <p className={`text-[13.5px] leading-relaxed text-text-hi ${fadeUp}`}>
                            {turn.assistantText}
                          </p>

                          {typeof turn.candidateCount === "number" && turn.candidateCount > 8 && (
                            <p
                              className={`font-mono text-[10.5px] uppercase tracking-wide text-text-lo/70 ${fadeUp}`}
                              style={live ? { animationDelay: "100ms" } : undefined}
                            >
                              {turn.candidateCount} games fit so far · narrowing down
                            </p>
                          )}

                          {isLastTurn && turn.assistantOptions && turn.assistantOptions.length > 0 && (
                            <div className="space-y-2.5">
                              <div className="flex flex-wrap gap-2">
                                {turn.assistantOptions.map((option, optionIdx) => {
                                  const selected = turn.assistantMultiSelect && selectedOptions.has(option);
                                  return (
                                    <button
                                      key={option}
                                      onClick={() =>
                                        turn.assistantMultiSelect ? toggleOption(option) : send(option)
                                      }
                                      disabled={loading}
                                      style={live ? { animationDelay: `${80 + optionIdx * 40}ms` } : undefined}
                                      className={`flex items-center gap-1.5 rounded-full border px-3.5 py-1.5 text-[12.5px] font-medium transition-all duration-150 disabled:opacity-50 ${fadeUp} ${
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
                                  style={
                                    live
                                      ? { animationDelay: `${80 + turn.assistantOptions.length * 40}ms` }
                                      : undefined
                                  }
                                  className={`rounded-full bg-text-hi px-4 py-1.5 text-[12.5px] font-semibold text-bg transition-opacity hover:opacity-85 disabled:cursor-not-allowed disabled:opacity-30 ${fadeUp}`}
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
                        <div
                          className={`mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-accent/10 text-accent ${fadeUp}`}
                        >
                          <Sparkles className="h-3 w-3" />
                        </div>
                        <div className="min-w-0 flex-1">
                          <p className={`text-[13.5px] leading-relaxed text-text-hi ${fadeUp}`}>
                            {turn.results.reasoning}
                          </p>
                          <div className="mt-4 grid grid-cols-2 gap-3.5 sm:grid-cols-3">
                            {turn.results.games.map((game, gameIdx) => (
                              <div
                                key={game.rawgId}
                                className={fadeUp || undefined}
                                style={live ? { animationDelay: `${100 + gameIdx * 55}ms` } : undefined}
                              >
                                <GameCard
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
                              </div>
                            ))}
                          </div>
                          {isLastTurn && (
                            <p
                              className={`mt-4 text-[12.5px] text-text-lo/80 ${fadeUp}`}
                              style={
                                live
                                  ? { animationDelay: `${100 + turn.results.games.length * 55 + 80}ms` }
                                  : undefined
                              }
                            >
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

              {/* Optimistic echo of the in-flight message — replaced in place (same markup,
                  no animation on the committed copy above) once the reply lands. */}
              {pending && (
                <div className="flex animate-fade-up justify-end">
                  <div className={USER_BUBBLE_CLASS}>{pending}</div>
                </div>
              )}

              {loading && <ThinkingBubble />}
              <div ref={bottomRef} />
            </div>
          </div>

          {(error || addError) && (
            <p className="mx-auto mt-2 max-w-2xl text-sm text-danger">{error ?? addError}</p>
          )}

          <form onSubmit={handleSubmit} className="mx-auto w-full max-w-2xl shrink-0 pb-1 pt-3">
            <div className="flex items-end gap-2.5 rounded-2xl border border-border-strong/60 bg-surface-alt/70 py-2.5 pl-5 pr-2 shadow-[0_8px_24px_-12px_rgba(0,0,0,0.5)] transition-all duration-200 hover:border-border-strong hover:shadow-[0_8px_24px_-12px_rgba(0,0,0,0.5),0_0_20px_-8px_rgba(185,106,85,0.35)] focus-within:border-accent/50 focus-within:shadow-[0_8px_24px_-12px_rgba(0,0,0,0.5),0_0_0_3px_rgba(185,106,85,0.12),0_0_28px_-8px_rgba(185,106,85,0.45)]">
              <textarea
                ref={textareaRef}
                value={input}
                onChange={handleInput}
                onKeyDown={handleKeyDown}
                rows={1}
                autoFocus
                placeholder="Message Shelby — a game, a genre, a mood..."
                style={{ maxHeight: TEXTAREA_MAX_HEIGHT }}
                className="block max-h-40 flex-1 resize-none overflow-y-auto bg-transparent py-2 text-sm leading-6 text-text-hi outline-none placeholder:text-text-lo/70"
              />
              <button
                type="submit"
                disabled={loading || !input.trim()}
                className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-accent text-bg transition-all duration-150 hover:scale-105 hover:bg-accent-hover enabled:active:scale-95 disabled:cursor-not-allowed disabled:bg-surface-alt disabled:text-text-lo disabled:hover:scale-100"
              >
                <Send className="h-4 w-4" />
              </button>
            </div>
            <p className="mt-2 text-center font-mono text-[10px] uppercase tracking-wide text-text-lo/50">
              Enter to send · Shift + Enter for a new line
            </p>
          </form>
        </div>
      </div>
    </div>
  );
}
