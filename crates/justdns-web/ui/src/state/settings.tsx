import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import {
  api,
  DEFAULT_BLOCK_PAGE,
  DEFAULT_NOTIFICATIONS,
  DEFAULT_REPORT_SETTINGS,
  type BlockPageSettings,
  type BlockResponse,
  type CustomTransport,
  type CustomUpstream,
  type NotificationSettings,
  type ServerSettings,
  type SpeedProfile,
  type UpstreamMode,
  type WebhookFormat,
} from "../lib/api";

export type {
  BlockPageSettings,
  BlockResponse,
  CustomTransport,
  CustomUpstream,
  NotificationSettings,
  SpeedProfile,
  UpstreamMode,
  WebhookFormat,
};

/// The full settings the UI works with: the server-persisted fields plus
/// `expertMode`, a per-browser preference about what the UI reveals that the
/// server has no reason to know about.
export interface Settings extends ServerSettings {
  expertMode: boolean;
}

/// Server defaults, mirroring the Rust `Settings::default`. Shown until the
/// first load returns, and as the fallback if the API is unreachable.
const SERVER_DEFAULTS: ServerSettings = {
  blockAdsTrackers: true,
  safeSearch: false,
  blockMalware: true,
  upstreamMode: "racing",
  encryptUpstream: true,
  speedProfile: "balanced",
  customUpstreamsEnabled: false,
  customUpstreams: [],
  ipv6: false,
  queryLog: true,
  logRetentionDays: 7,
  instanceName: "JustDNS",
  blockResponse: "null-ip",
  activeWindowSecs: 300,
  timezone: "UTC",
  requireMfa: false,
  reports: DEFAULT_REPORT_SETTINGS,
  blockPage: DEFAULT_BLOCK_PAGE,
  notifications: DEFAULT_NOTIFICATIONS,
};

const EXPERT_KEY = "justdns.expertMode";
const SAVE_DEBOUNCE_MS = 400;

const SERVER_KEYS: (keyof ServerSettings)[] = [
  "blockAdsTrackers",
  "safeSearch",
  "blockMalware",
  "upstreamMode",
  "encryptUpstream",
  "speedProfile",
  "customUpstreamsEnabled",
  "customUpstreams",
  "ipv6",
  "queryLog",
  "logRetentionDays",
  "instanceName",
  "blockResponse",
  "activeWindowSecs",
  "timezone",
  "requireMfa",
  "reports",
  "blockPage",
  "notifications",
];

function toServer(settings: Settings): ServerSettings {
  return SERVER_KEYS.reduce((acc, key) => {
    (acc[key] as ServerSettings[typeof key]) = settings[key];
    return acc;
  }, {} as ServerSettings);
}

function loadExpertMode(): boolean {
  try {
    return localStorage.getItem(EXPERT_KEY) === "true";
  } catch {
    return false;
  }
}

interface SettingsContextValue {
  settings: Settings;
  update: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
}

const SettingsContext = createContext<SettingsContextValue | null>(null);

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings>(() => ({
    ...SERVER_DEFAULTS,
    expertMode: loadExpertMode(),
  }));

  // Load the server-persisted settings once; keep the local expertMode.
  useEffect(() => {
    const controller = new AbortController();
    api
      .settings(controller.signal)
      .then((server) => setSettings((prev) => ({ ...prev, ...server })))
      .catch(() => {
        // Unreachable API: keep the defaults so the page still works.
      });
    return () => controller.abort();
  }, []);

  const saveTimer = useRef<number | undefined>(undefined);

  const scheduleSave = useCallback((next: Settings) => {
    if (saveTimer.current !== undefined) {
      window.clearTimeout(saveTimer.current);
    }
    saveTimer.current = window.setTimeout(() => {
      api.saveSettings(toServer(next)).catch(() => {
        // A failed save leaves the optimistic UI in place; the next change
        // retries.
      });
    }, SAVE_DEBOUNCE_MS);
  }, []);

  const update = useCallback<SettingsContextValue["update"]>(
    (key, value) => {
      setSettings((prev) => {
        const next = { ...prev, [key]: value };
        if (key === "expertMode") {
          try {
            localStorage.setItem(EXPERT_KEY, value ? "true" : "false");
          } catch {
            // A browser that refuses storage still gets a working session.
          }
        } else {
          scheduleSave(next);
        }
        return next;
      });
    },
    [scheduleSave],
  );

  const value = useMemo(() => ({ settings, update }), [settings, update]);

  return <SettingsContext.Provider value={value}>{children}</SettingsContext.Provider>;
}

export function useSettings(): SettingsContextValue {
  const ctx = useContext(SettingsContext);
  if (!ctx) throw new Error("useSettings must be used within SettingsProvider");
  return ctx;
}
