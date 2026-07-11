import { Routes, Route } from "react-router-dom";
import { Shell } from "./components/Layout/Shell";
import { Dashboard } from "./components/Dashboard/Dashboard";
import { Library } from "./components/Library/Library";
import { GameDetail } from "./components/Library/GameDetail";
import { Discover } from "./components/Discover/Discover";
import { DiscoverChat } from "./components/Discover/DiscoverChat";
import { Clips } from "./components/Clips/Clips";
import { Settings } from "./components/Settings/Settings";
import { OverlayToast } from "./components/Overlay/OverlayToast";

function App() {
  return (
    <Routes>
      {/* Rendered inside the separate in-game overlay window, not the main app shell. */}
      <Route path="overlay" element={<OverlayToast />} />
      <Route element={<Shell />}>
        <Route index element={<Dashboard />} />
        <Route path="library" element={<Library />} />
        <Route path="library/:id" element={<GameDetail />} />
        <Route path="discover" element={<Discover />} />
        <Route path="discover/chat" element={<DiscoverChat />} />
        <Route path="clips" element={<Clips />} />
        <Route path="settings" element={<Settings />} />
      </Route>
    </Routes>
  );
}

export default App;
