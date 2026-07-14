import { create } from "zustand";
import type { RawgGameResult } from "../components/shared/GameCard";

export type GameStatus = "backlog" | "playing" | "completed" | "dropped" | "wishlist";

export interface RecommendedGame extends RawgGameResult {
  reason?: string | null;
}

export interface ChatTurn {
  user: string;
  assistantText?: string;
  assistantOptions?: string[];
  assistantMultiSelect?: boolean;
  // Real size of the AI's remaining candidate pool when it asked this question — shown as
  // honest narrowing progress ("14 candidates left"), absent when unknown.
  candidateCount?: number | null;
  results?: { reasoning: string; games: RecommendedGame[] };
}

export interface Game {
  id: number;
  rawgId: number | null;
  name: string;
  coverUrl: string | null;
  genre: string | null;
  platform: string | null;
  status: GameStatus;
  rating: number | null;
  notes: string | null;
  exeName: string | null;
  addedAt: string;
  completedAt: string | null;
}

interface AppState {
  // Discover "Ask AI" chat state, lifted out of the component so it survives route
  // navigation and tab switches instead of resetting every time ChatTab unmounts.
  chatTurns: ChatTurn[];
  chatQuestionsAsked: number;
  setChatTurns: (turns: ChatTurn[] | ((prev: ChatTurn[]) => ChatTurn[])) => void;
  setChatQuestionsAsked: (count: number | ((prev: number) => number)) => void;
  // Row id of the active conversation in chat_conversations — null until its first
  // save (a brand-new chat), set/cleared by DiscoverChat as chats are loaded/reset.
  chatId: number | null;
  setChatId: (id: number | null) => void;
  // True once DiscoverChat has attempted to restore the most recent conversation from
  // the DB this app-run — keeps "New chat" from being clobbered by a re-restore.
  chatHydrated: boolean;
  setChatHydrated: (hydrated: boolean) => void;
  // The For You set as currently shown (base fetch + any "Load more" batches). Lives here
  // so hopping Discover ↔ Shelby keeps the exact same grid; Shell clears it when the user
  // leaves the Discover section entirely, which is what triggers a rediscover.
  forYouRecs: { reasoning: string; games: RecommendedGame[] } | null;
  setForYouRecs: (recs: { reasoning: string; games: RecommendedGame[] } | null) => void;
  // Version string of an available in-app update (TopNav's startup check sets it; the
  // Settings nav item shows a dot, and the About card offers the install).
  updateAvailable: string | null;
  setUpdateAvailable: (version: string | null) => void;
  // The clip hotkey as registered (task 26) — loaded once by Shell, updated by Settings.
  // Every piece of copy that names the hotkey reads this instead of hardcoding Alt+F9.
  clipHotkey: string;
  setClipHotkey: (hotkey: string) => void;
  // The global quick-open palette (Ctrl/⌘+K) — opened by the shortcut or TopNav's search
  // button, rendered once by Shell so it works from every page.
  quickOpenVisible: boolean;
  setQuickOpenVisible: (visible: boolean) => void;
  // One-line "first ever" unlock banner (first session / clip / completed game). Shell
  // renders it app-wide; unlocks.ts sets it at most once per lifetime event.
  unlockNotice: string | null;
  setUnlockNotice: (notice: string | null) => void;
}

export const useAppStore = create<AppState>((set) => ({
  chatTurns: [],
  chatQuestionsAsked: 0,
  setChatTurns: (turns) =>
    set((state) => ({ chatTurns: typeof turns === "function" ? turns(state.chatTurns) : turns })),
  setChatQuestionsAsked: (count) =>
    set((state) => ({
      chatQuestionsAsked: typeof count === "function" ? count(state.chatQuestionsAsked) : count,
    })),
  chatId: null,
  setChatId: (id) => set({ chatId: id }),
  chatHydrated: false,
  setChatHydrated: (hydrated) => set({ chatHydrated: hydrated }),
  forYouRecs: null,
  setForYouRecs: (recs) => set({ forYouRecs: recs }),
  updateAvailable: null,
  setUpdateAvailable: (version) => set({ updateAvailable: version }),
  clipHotkey: "Alt+F9",
  setClipHotkey: (hotkey) => set({ clipHotkey: hotkey }),
  quickOpenVisible: false,
  setQuickOpenVisible: (visible) => set({ quickOpenVisible: visible }),
  unlockNotice: null,
  setUnlockNotice: (notice) => set({ unlockNotice: notice }),
}));
