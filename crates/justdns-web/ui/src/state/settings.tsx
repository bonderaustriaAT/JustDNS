import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

export type UpstreamMode = "racing" | "fallback" | "balanced" | "rules";

export interface Settings {
  expertMode: boolean;
  blockAdsTrackers: boolean;
  safeSearch: boolean;
  blockMalware: boolean;
  // Expert-only, mirrors the DNS core's options.
  upstreamMode: UpstreamMode;
  encryptUpstream: boolean;
  ipv6: boolean;
  queryLog: boolean;
  logRetentionDays: number;
  cacheServeStale: boolean;
}

const DEFAULTS: Settings = {
  expertMode: false,
  blockAdsTrackers: true,
  safeSearch: false,
  blockMalware: true,
  upstreamMode: "racing",
  encryptUpstream: true,
  ipv6: false,
  queryLog: true,
  logRetentionDays: 7,
  cacheServeStale: false,
};

const STORAGE_KEY = "justdns.settings";

interface SettingsContextValue {
  settings: Settings;
  update: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
}

const SettingsContext = createContext<SettingsContextValue | null>(null);

function load(): Settings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULTS;
    return { ...DEFAULTS, ...(JSON.parse(raw) as Partial<Settings>) };
  } catch {
    return DEFAULTS;
  }
}

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings>(load);

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
    } catch {
      // A browser that refuses storage still gets a working session.
    }
  }, [settings]);

  const update = useCallback<SettingsContextValue["update"]>((key, value) => {
    setSettings((prev) => ({ ...prev, [key]: value }));
  }, []);

  const value = useMemo(() => ({ settings, update }), [settings, update]);

  return <SettingsContext.Provider value={value}>{children}</SettingsContext.Provider>;
}

export function useSettings(): SettingsContextValue {
  const ctx = useContext(SettingsContext);
  if (!ctx) throw new Error("useSettings must be used within SettingsProvider");
  return ctx;
}
