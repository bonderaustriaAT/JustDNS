import { Navigate, Route, Routes } from "react-router-dom";

import { Shell } from "./components/Shell";
import { AccessRequests } from "./pages/AccessRequests";
import { Account } from "./pages/Account";
import { AuditLog } from "./pages/AuditLog";
import { BlockedServices } from "./pages/BlockedServices";
import { Certificates } from "./pages/Certificates";
import { Clients } from "./pages/Clients";
import { Dnssec } from "./pages/Dnssec";
import { HighAvailability } from "./pages/HighAvailability";
import { Lists } from "./pages/Lists";
import { Login } from "./pages/Login";
import { Logs } from "./pages/Logs";
import { Overview } from "./pages/Overview";
import { Reports } from "./pages/Reports";
import { Rewrites } from "./pages/Rewrites";
import { Settings } from "./pages/Settings";
import { SetupWizard } from "./pages/SetupWizard";
import { SystemLogs } from "./pages/SystemLogs";
import { Users } from "./pages/Users";
import { Zeitplaene } from "./pages/Zeitplaene";
import { useAuth } from "./state/auth";
import { SettingsProvider } from "./state/settings";

export function App() {
  const { status, configured, authenticated, isAdmin } = useAuth();

  // Hold the whole UI back until we know whether a login is required, so the
  // dashboard never flashes before the gate decides.
  if (status === "loading") return null;

  // First run: no admin yet, so run the setup wizard. Otherwise require a login.
  if (!configured) return <SetupWizard />;
  if (!authenticated) return <Login />;

  // The settings only load once a login is in place, so their guarded fetch
  // never fires on the login screen.
  return (
    <SettingsProvider>
      <Shell>
        <Routes>
          <Route path="/" element={<Overview />} />
          <Route path="/protokoll" element={<Logs />} />
          <Route path="/dnssec" element={<Dnssec />} />
          <Route path="/geraete" element={<Clients />} />
          <Route path="/listen" element={<Navigate to="/listen/url" replace />} />
          <Route path="/listen/eigene" element={<Navigate to="/listen/url" replace />} />
          <Route path="/listen/url" element={<Lists section="mine" />} />
          <Route path="/listen/fertige" element={<Lists section="presets" />} />
          <Route
            path="/dienste"
            element={isAdmin ? <BlockedServices /> : <Navigate to="/" replace />}
          />
          <Route path="/zeitplaene" element={<Zeitplaene />} />
          <Route path="/rewrites" element={<Rewrites />} />
          <Route path="/einstellungen" element={<Settings />} />
          <Route
            path="/zertifikate"
            element={isAdmin ? <Certificates /> : <Navigate to="/" replace />}
          />
          <Route
            path="/freigaben"
            element={isAdmin ? <AccessRequests /> : <Navigate to="/" replace />}
          />
          <Route path="/konto" element={<Account />} />
          {/* Admin-only areas: fall back to the overview for anyone else. */}
          <Route path="/benutzer" element={isAdmin ? <Users /> : <Navigate to="/" replace />} />
          <Route path="/audit" element={isAdmin ? <AuditLog /> : <Navigate to="/" replace />} />
          <Route path="/system" element={isAdmin ? <SystemLogs /> : <Navigate to="/" replace />} />
          <Route path="/berichte" element={isAdmin ? <Reports /> : <Navigate to="/" replace />} />
          <Route
            path="/ha"
            element={isAdmin ? <HighAvailability /> : <Navigate to="/" replace />}
          />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </Shell>
    </SettingsProvider>
  );
}
