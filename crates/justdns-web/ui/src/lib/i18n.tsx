// A small, dependency-free internationalisation layer. German is the source
// language and the fallback; English is the second catalogue. The whole UI reads
// its text through `t()` from `useI18n()`, so adding a language later means
// adding a catalogue in `messages.ts` and a `Lang` value here — nothing else.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import { messages, type MessageKey } from "./messages";

export type Lang = "de" | "en";

/// The languages offered in the switcher, in display order.
export const LANGS: Lang[] = ["de", "en"];

/// Where the chosen language is remembered between visits.
const STORAGE_KEY = "justdns.lang";
/// The fallback whenever nothing else is known; also the catalogue that backs
/// every key, so a missing translation shows German rather than the raw key.
const DEFAULT_LANG: Lang = "de";

/// Values substituted into `{placeholder}` slots in a message.
export type Vars = Record<string, string | number>;

/// A module-level mirror of the active language, so non-React callers (the plain
/// label helpers in `dns.ts`) can translate without a hook. The provider keeps
/// it in step with React state.
let currentLang: Lang = DEFAULT_LANG;

/// The active language for code that runs outside the React tree.
export function getLang(): Lang {
  return currentLang;
}

/// Look up `key` in `lang`'s catalogue, fall back to German, then to the raw
/// key, and fill any `{name}` placeholders from `vars`.
export function translate(key: MessageKey, vars?: Vars, lang: Lang = currentLang): string {
  const table = messages[lang] ?? messages[DEFAULT_LANG];
  const template = table[key] ?? messages[DEFAULT_LANG][key] ?? key;
  if (!vars) {
    return template;
  }
  return template.replace(/\{(\w+)\}/g, (whole, name: string) => {
    const value = vars[name];
    return value === undefined ? whole : String(value);
  });
}

/// The BCP-47 locale used for number and date formatting per language.
function localeFor(lang: Lang): string {
  return lang === "de" ? "de-DE" : "en-US";
}

/// Pick the starting language: a remembered choice wins, otherwise the browser's
/// preference if it is one we offer, otherwise German.
function initialLang(): Lang {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "de" || stored === "en") {
      return stored;
    }
  } catch {
    // localStorage can be unavailable (private mode); fall through to detection.
  }
  const navLang = typeof navigator !== "undefined" ? navigator.language.toLowerCase() : "";
  if (navLang.startsWith("en")) {
    return "en";
  }
  return DEFAULT_LANG;
}

interface I18n {
  lang: Lang;
  setLang: (lang: Lang) => void;
  t: (key: MessageKey, vars?: Vars) => string;
  locale: string;
}

const I18nContext = createContext<I18n | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(() => {
    const initial = initialLang();
    currentLang = initial;
    return initial;
  });

  const setLang = useCallback((next: Lang) => {
    currentLang = next;
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // Persisting the choice is best-effort; the UI still switches.
    }
    setLangState(next);
  }, []);

  // Reflect the language on the document so the page element matches and any
  // language-specific CSS or screen-reader behaviour can key off it.
  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);

  const value = useMemo<I18n>(
    () => ({
      lang,
      setLang,
      t: (key, vars) => translate(key, vars, lang),
      locale: localeFor(lang),
    }),
    [lang, setLang],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18n {
  const value = useContext(I18nContext);
  if (!value) {
    throw new Error("useI18n must be used within an I18nProvider");
  }
  return value;
}
