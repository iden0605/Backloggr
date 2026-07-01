import { create } from "zustand";

export type GameStatus = "backlog" | "playing" | "completed" | "dropped" | "wishlist";

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
  games: Game[];
  currentlyPlayingId: number | null;
  setGames: (games: Game[]) => void;
  setCurrentlyPlayingId: (id: number | null) => void;
}

export const useAppStore = create<AppState>((set) => ({
  games: [],
  currentlyPlayingId: null,
  setGames: (games) => set({ games }),
  setCurrentlyPlayingId: (id) => set({ currentlyPlayingId: id }),
}));
