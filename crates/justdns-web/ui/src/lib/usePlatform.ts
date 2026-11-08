import { useEffect, useState } from "react";

/// What we need to know about the device to tailor the install experience: iOS
/// and Android get different install flows, and an already-installed (standalone)
/// app should never be nagged to install again. Layout stays viewport-driven;
/// this only steers the install hint.
export interface Platform {
  isIOS: boolean;
  isAndroid: boolean;
  isStandalone: boolean;
}

/// `navigator.standalone` is a non-standard iOS-only flag for a home-screen app.
type IosNavigator = Navigator & { standalone?: boolean };

function detect(): Platform {
  if (typeof navigator === "undefined") {
    return { isIOS: false, isAndroid: false, isStandalone: false };
  }
  const ua = navigator.userAgent || "";
  // iPadOS 13+ presents a desktop Safari UA, so treat a touch-capable "Mac" as
  // iOS too.
  const isIOS =
    /iphone|ipad|ipod/i.test(ua) ||
    (/\bMacintosh\b/.test(ua) && typeof document !== "undefined" && "ontouchend" in document);
  const isAndroid = /android/i.test(ua);
  const standalone =
    (typeof window !== "undefined" &&
      window.matchMedia?.("(display-mode: standalone)").matches) ||
    (navigator as IosNavigator).standalone === true;
  return { isIOS, isAndroid, isStandalone: Boolean(standalone) };
}

/// Re-reads when the display mode flips (e.g. the app is launched standalone).
export function usePlatform(): Platform {
  const [platform, setPlatform] = useState<Platform>(detect);
  useEffect(() => {
    const mq = window.matchMedia?.("(display-mode: standalone)");
    if (!mq) return;
    const onChange = () => setPlatform(detect());
    mq.addEventListener?.("change", onChange);
    return () => mq.removeEventListener?.("change", onChange);
  }, []);
  return platform;
}
