import { Navigate, Route, Routes } from "react-router-dom";

import { Shell } from "./components/Shell";
import { CreateAdmin } from "./pages/CreateAdmin";
import { Login } from "./pages/Login";
import { Logs } from "./pages/Logs";
import { Overview } from "./pages/Overview";
import { Settings } from "./pages/Settings";
import { useAuth } from "./state/auth";
import { SettingsProvider } from "./state/settings";

export function App() {
  const { status, configured, authenticated } = useAuth();

  // Hold the whole UI back until we know whether a login is required, so the
  // dashboard never flashes before the gate decides.
  if (status === "loading") return null;

  // First run: no admin yet, so offer to create one. Otherwise require a login.
  if (!configured) return <CreateAdmin />;
  if (!authenticated) return <Login />;

  // The settings only load once a login is in place, so their guarded fetch
  // never fires on the login screen.
  return (
    <SettingsProvider>
      <Shell>
        <Routes>
          <Route path="/" element={<Overview />} />
          <Route path="/protokoll" element={<Logs />} />
          <Route path="/einstellungen" element={<Settings />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </Shell>
    </SettingsProvider>
  );
}
