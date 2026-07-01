import { useEffect } from "react";
import { Routes, Route } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { Shell } from "./components/Layout/Shell";
import { Dashboard } from "./components/Dashboard/Dashboard";
import { Backlog } from "./components/Backlog/Backlog";
import { Search } from "./components/Search/Search";
import { Recommendations } from "./components/Recommendations/Recommendations";
import { Clips } from "./components/Clips/Clips";
import { Settings } from "./components/Settings/Settings";
import { useAppStore } from "./store/useAppStore";

interface SessionStarted {
  gameId: number;
}
interface SessionEnded {
  gameId: number;
}

function App() {
  const setCurrentlyPlayingId = useAppStore((s) => s.setCurrentlyPlayingId);

  useEffect(() => {
    const unlistenStarted = listen<SessionStarted>("session-started", (event) => {
      setCurrentlyPlayingId(event.payload.gameId);
    });
    const unlistenEnded = listen<SessionEnded>("session-ended", () => {
      setCurrentlyPlayingId(null);
    });

    return () => {
      unlistenStarted.then((f) => f());
      unlistenEnded.then((f) => f());
    };
  }, [setCurrentlyPlayingId]);

  return (
    <Routes>
      <Route element={<Shell />}>
        <Route index element={<Dashboard />} />
        <Route path="backlog" element={<Backlog />} />
        <Route path="search" element={<Search />} />
        <Route path="recommendations" element={<Recommendations />} />
        <Route path="clips" element={<Clips />} />
        <Route path="settings" element={<Settings />} />
      </Route>
    </Routes>
  );
}

export default App;
