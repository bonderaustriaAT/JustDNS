import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import { api, setUnauthorizedHandler, type LoginStatus, type Role } from "../lib/api";

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
  /// The signed-in user's role, or null when logged out.
  role: Role | null;
  /// Full control (user management, settings, TLS, backups).
  isAdmin: boolean;
  /// May change filtering content (admins and editors) — and only when this node
  /// is writable (the primary, or no primary designated). On a read-only follower
  /// this is always false, so every edit control in the app hides at once.
  canEdit: boolean;
  /// `false` only on a read-only follower of a live primary node.
  isPrimary: boolean;
  /// The primary node's URL, shown to a follower so the user knows where to manage.
  primaryUrl: string | null;
  /// Create the first administrator and log in. Throws on failure.
  setup: (username: string, password: string) => Promise<void>;
  /// Mark setup finished locally after the wizard has already created the admin
  /// (via the API) and applied its configuration. Flipping the gate only at the
  /// end keeps the wizard mounted across its own steps.
  completeSetup: (username: string) => void;
  /// Step one of signing in: check the password. The returned status says
  /// whether the login is finished or a second factor is still needed. Throws on
  /// a wrong password.
  login: (username: string, password: string) => Promise<LoginStatus>;
  /// Finalise the session after a second-factor step (MFA or enrolment) has
  /// promoted it on the server, refreshing who we are.
  completeLogin: () => Promise<void>;
  /// Log out and return to the login screen.
  logout: () => Promise<void>;
}

const AuthContext = createContext<AuthContextValue | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<"loading" | "ready">("loading");
  const [configured, setConfigured] = useState(false);
  const [authenticated, setAuthenticated] = useState(false);
  const [username, setUsername] = useState<string | null>(null);
  const [role, setRole] = useState<Role | null>(null);
  // Default to writable: an older server without the field, or an unreachable one,
  // behaves exactly as before this feature rather than locking the UI.
  const [isPrimary, setIsPrimary] = useState(true);
  const [primaryUrl, setPrimaryUrl] = useState<string | null>(null);

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
        setRole(state.role ?? null);
        setIsPrimary(state.isPrimary ?? true);
        setPrimaryUrl(state.primaryUrl ?? null);
      })
      .catch(() => {
        setConfigured(true);
        setAuthenticated(false);
        setUsername(null);
        setRole(null);
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
      setRole(null);
    });
    return () => setUnauthorizedHandler(null);
  }, []);

  // Re-read who we are from the server — used after a login finishes (including
  // a second factor), so the role gating is correct from the first render.
  const finalize = useCallback(async () => {
    const state = await api.authState();
    setConfigured(state.configured);
    setAuthenticated(state.authenticated);
    setUsername(state.username);
    setRole(state.role ?? null);
    setIsPrimary(state.isPrimary ?? true);
    setPrimaryUrl(state.primaryUrl ?? null);
  }, []);

  const setup = useCallback(async (user: string, password: string) => {
    await api.setup(user, password);
    // The first account is always an administrator.
    setConfigured(true);
    setAuthenticated(true);
    setUsername(user);
    setRole("admin");
  }, []);

  const completeSetup = useCallback((user: string) => {
    setConfigured(true);
    setAuthenticated(true);
    setUsername(user);
    setRole("admin");
  }, []);

  const login = useCallback(
    async (user: string, password: string): Promise<LoginStatus> => {
      const result = await api.login(user, password);
      if (result.status === "authenticated") {
        await finalize();
      }
      return result.status;
    },
    [finalize],
  );

  const completeLogin = useCallback(async () => {
    await finalize();
  }, [finalize]);

  const logout = useCallback(async () => {
    try {
      await api.logout();
    } finally {
      // Even if the request fails, clear the local view so the user is not
      // stuck on a dashboard they think they left.
      setAuthenticated(false);
      setUsername(null);
      setRole(null);
    }
  }, []);

  const value = useMemo<AuthContextValue>(
    () => ({
      status,
      configured,
      authenticated,
      username,
      role,
      isAdmin: role === "admin",
      canEdit: (role === "admin" || role === "editor") && isPrimary,
      isPrimary,
      primaryUrl,
      setup,
      completeSetup,
      login,
      completeLogin,
      logout,
    }),
    [
      status,
      configured,
      authenticated,
      username,
      role,
      isPrimary,
      primaryUrl,
      setup,
      completeSetup,
      login,
      completeLogin,
      logout,
    ],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}
