import { useEffect, useState } from "react";

import { useI18n } from "../lib/i18n";
import { usePlatform } from "../lib/usePlatform";
import styles from "./InstallBanner.module.css";

const DISMISS_KEY = "justdns.installDismissed";

/// The browser's install event, offered on Android and desktop Chromium. It is
/// not in the DOM lib types, so we describe the bits we use.
interface BeforeInstallPromptEvent extends Event {
  prompt: () => Promise<void>;
  userChoice: Promise<{ outcome: "accepted" | "dismissed" }>;
}

/// A quiet, dismissible prompt to install JustDNS as an app. On Android/desktop
/// Chromium it offers a real install button (via `beforeinstallprompt`); on iOS
/// Safari, which has no such event, it shows the Share → Add to Home Screen hint.
/// Hidden once installed (standalone) or dismissed.
export function InstallBanner() {
  const { t } = useI18n();
  const { isIOS, isStandalone } = usePlatform();
  const [deferred, setDeferred] = useState<BeforeInstallPromptEvent | null>(null);
  const [dismissed, setDismissed] = useState<boolean>(() => {
    try {
      return localStorage.getItem(DISMISS_KEY) === "true";
    } catch {
      return false;
    }
  });

  useEffect(() => {
    const onPrompt = (event: Event) => {
      event.preventDefault(); // Keep our own affordance instead of the mini-infobar.
      setDeferred(event as BeforeInstallPromptEvent);
    };
    const onInstalled = () => {
      setDeferred(null);
      dismiss();
    };
    window.addEventListener("beforeinstallprompt", onPrompt);
    window.addEventListener("appinstalled", onInstalled);
    return () => {
      window.removeEventListener("beforeinstallprompt", onPrompt);
      window.removeEventListener("appinstalled", onInstalled);
    };
  }, []);

  function dismiss() {
    setDismissed(true);
    try {
      localStorage.setItem(DISMISS_KEY, "true");
    } catch {
      // A browser that refuses storage simply re-shows the hint next time.
    }
  }

  const canPrompt = deferred !== null;
  // Nothing to offer: already installed, dismissed, or a browser that neither
  // fires the install event nor is iOS Safari.
  if (isStandalone || dismissed || (!canPrompt && !isIOS)) return null;

  async function install() {
    if (!deferred) return;
    await deferred.prompt();
    await deferred.userChoice.catch(() => undefined);
    setDeferred(null);
    dismiss();
  }

  return (
    <div className={styles.banner} role="region" aria-label={t("pwa.installTitle")}>
      <div className={styles.text}>
        <span className={styles.title}>{t("pwa.installTitle")}</span>
        {isIOS && !canPrompt ? <span className={styles.hint}>{t("pwa.installIosHint")}</span> : null}
      </div>
      <div className={styles.actions}>
        {canPrompt ? (
          <button type="button" className={styles.install} onClick={install}>
            {t("pwa.install")}
          </button>
        ) : null}
        <button type="button" className={styles.later} onClick={dismiss}>
          {t("pwa.later")}
        </button>
      </div>
    </div>
  );
}
