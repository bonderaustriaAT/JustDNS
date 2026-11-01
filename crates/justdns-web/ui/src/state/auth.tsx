import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import { api, setUnauthorizedHandler } from "../lib/api";

/// What the whole app needs to know about who is signed in. `status` drives the
/// top-level gate: while it is "loading" nothing is shown, so the dashboard
/// never flashes before we know whether a login is required.
interface AuthContextValue {
  status: "loading" | "ready";
  /// Whether an administrator account exists yet.
  configured: boolean;
  /// Whether the current browser is logged in.
  authenticated: boolean;
  username: string | null;
  /// Create the first administrator and log in. Throws on failure.
  setup: (username: string, password: string) => Promise<void>;
  /// Mark setup finished locally after the wizard has already created the admin
  /// (via the API) and applied its configuration. Flipping the gate only at the
  /// end keeps the wizard mounted across its own steps.
  completeSetup: (username: string) => void;
  /// Log in with an existing account. Throws on a wrong password.
  login: (username: string, password: string) => Promise<void>;
  /// Log out and return to the login screen.
  logout: () => Promise<void>;
}

const AuthContext = createContext<AuthContextValue | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<"loading" | "ready">("loading");
  const [configured, setConfigured] = useState(false);
  const [authenticated, setAuthenticated] = useState(false);
  const [username, setUsername] = useState<string | null>(null);

  // Ask the server once on load who we are. If the API is unreachable we treat
  // the box as configured-but-logged-out, so the user sees the login screen
  // rather than the (destructive) first-run setup form.
  useEffect(() => {
    const controller = new AbortController();
    api
      .authState(controller.signal)
      .then((state) => {
        setConfigured(state.configured);
        setAuthenticated(state.authenticated);
        setUsername(state.username);
      })
      .catch(() => {
        setConfigured(true);
        setAuthenticated(false);
        setUsername(null);
      })
      .finally(() => setStatus("ready"));
    return () => controller.abort();
  }, []);

  // A guarded call that comes back 401 means the session lapsed: drop to the
  // login screen without a reload.
  useEffect(() => {
    setUnauthorizedHandler(() => {
      setAuthenticated(false);
      setUsername(null);
    });
    return () => setUnauthorizedHandler(null);
  }, []);

  const setup = useCallback(async (user: string, password: string) => {
    const result = await api.setup(user, password);
    setConfigured(true);
    setAuthenticated(true);
    setUsername(result.username);
  }, []);

  const completeSetup = useCallback((user: string) => {
    setConfigured(true);
    setAuthenticated(true);
    setUsername(user);
  }, []);

  const login = useCallback(async (user: string, password: string) => {
    const result = await api.login(user, password);
    setAuthenticated(true);
    setUsername(result.username);
  }, []);

  const logout = useCallback(async () => {
    try {
      await api.logout();
    } finally {
      // Even if the request fails, clear the local view so the user is not
      // stuck on a dashboard they think they left.
      setAuthenticated(false);
      setUsername(null);
    }
  }, []);

  const value = useMemo<AuthContextValue>(
    () => ({ status, configured, authenticated, username, setup, completeSetup, login, logout }),
    [status, configured, authenticated, username, setup, completeSetup, login, logout],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}
