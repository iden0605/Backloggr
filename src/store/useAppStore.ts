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
  currentlyPlayingId: number | null;
  setCurrentlyPlayingId: (id: number | null) => void;
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
}

export const useAppStore = create<AppState>((set) => ({
  currentlyPlayingId: null,
  setCurrentlyPlayingId: (id) => set({ currentlyPlayingId: id }),
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
}));
