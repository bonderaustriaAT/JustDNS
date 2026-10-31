import { Navigate, Route, Routes } from "react-router-dom";

import { Shell } from "./components/Shell";
import { Overview } from "./pages/Overview";
import { Settings } from "./pages/Settings";

export function App() {
  return (
    <Shell>
      <Routes>
        <Route path="/" element={<Overview />} />
        <Route path="/einstellungen" element={<Settings />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </Shell>
  );
}
