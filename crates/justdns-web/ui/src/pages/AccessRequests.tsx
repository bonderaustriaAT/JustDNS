// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useMemo, useState } from "react";

import { ConfirmDialog } from "../components/ConfirmDialog";
import { ApiError, api, type AccessRequest, type ReleaseScope } from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import styles from "./AccessRequests.module.css";

/** Map a block-category slug to its label key; an unknown slug shows verbatim. */
const CATEGORY_LABEL: Record<string, MessageKey> = {
  ads: "access.category.ads",
  malware: "access.category.malware",
  custom: "access.category.custom",
};

/** "From" line as device + optional group, falling back to the bare address. */
function who(req: AccessRequest, t: ReturnType<typeof useI18n>["t"]): string {
  const name = req.device ?? t("access.deviceUnknown");
  return req.group ? `${name} · ${t("access.inGroup", { name: req.group })}` : name;
}

/** A relative "n minutes ago" plus the absolute timestamp as a tooltip/second
 *  line, mirroring the wording used elsewhere in the dashboard. */
function when(ms: number, t: ReturnType<typeof useI18n>["t"]): string {
  const seconds = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (seconds < 5) return t("time.justNow");
  if (seconds < 60) return t("time.secondsAgo", { n: seconds });
  const minutes = Math.round(seconds / 60);
  if (minutes === 1) return t("time.oneMinuteAgo");
  if (minutes < 60) return t("time.minutesAgo", { n: minutes });
  const hours = Math.round(minutes / 60);
  if (hours < 24) return hours === 1 ? t("time.oneHourAgo") : t("time.hoursAgo", { n: hours });
  const days = Math.round(hours / 24);
  return days === 1 ? t("time.oneDayAgo") : t("time.daysAgo", { n: days });
}

/** What blocked the domain: the custom list name when known, otherwise the
 *  category label, with the exact rule appended when it differs from the domain. */
function blockedBy(req: AccessRequest, t: ReturnType<typeof useI18n>["t"]): string {
  const categoryKey = req.blockCategory ? CATEGORY_LABEL[req.blockCategory] : undefined;
  const source = req.blockList ?? (categoryKey ? t(categoryKey) : (req.blockCategory ?? "—"));
  if (req.blockRule && req.blockRule !== req.domain) {
    return `${source} (${req.blockRule})`;
  }
  return source;
}

/** One request, with the scope picker and the release / dismiss actions. */
function RequestCard({
  req,
  onDone,
}: {
  req: AccessRequest;
  onDone: (id: string) => void;
}) {
  const { t, locale } = useI18n();
  // A request with no resolved device can only be released for everyone, so that
  // is both the default and the only enabled scope there.
  const hasDevice = Boolean(req.device);
  const [scope, setScope] = useState<ReleaseScope>(hasDevice ? "device" : "all");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmAll, setConfirmAll] = useState(false);

  const scopeHint =
    scope === "device"
      ? t("access.scope.deviceHint")
      : scope === "group"
        ? t("access.scope.groupHint", { name: req.group ?? "" })
        : t("access.scope.allHint");

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      onDone(req.id);
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("access.actionError"));
      setBusy(false);
    }
  }

  function release() {
    // A global, permanent release is broad enough to confirm first.
    if (scope === "all") {
      setConfirmAll(true);
      return;
    }
    void run(() => api.releaseAccessRequest(req.id, scope));
  }

  const absolute = new Date(req.createdMs).toLocaleString(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  });
  const categoryKey = req.blockCategory ? CATEGORY_LABEL[req.blockCategory] : undefined;

  const scopes: ReleaseScope[] = ["device", "group", "all"];

  return (
    <article className={styles.card}>
      <header className={styles.cardHead}>
        <span className={styles.domain}>{req.domain}</span>
        {(categoryKey || req.blockCategory) && (
          <span className={styles.badge} data-category={req.blockCategory ?? "custom"}>
            {categoryKey ? t(categoryKey) : req.blockCategory}
          </span>
        )}
      </header>

      <dl className={styles.meta}>
        <div className={styles.metaRow}>
          <dt>{t("access.fromDevice")}</dt>
          <dd>{who(req, t)}</dd>
        </div>
        <div className={styles.metaRow}>
          <dt>{t("access.when")}</dt>
          <dd>
            {when(req.createdMs, t)} <span className={styles.absolute}>· {absolute}</span>
          </dd>
        </div>
        <div className={styles.metaRow}>
          <dt>{t("access.ip")}</dt>
          <dd className={styles.mono}>{req.clientIp}</dd>
        </div>
        <div className={styles.metaRow}>
          <dt>{t("access.blockedBy")}</dt>
          <dd>{blockedBy(req, t)}</dd>
        </div>
      </dl>

      <blockquote className={styles.reason} data-empty={req.reason ? undefined : true}>
        {req.reason ?? t("access.noReason")}
      </blockquote>

      <div className={styles.scopeBar} role="group" aria-label={t("access.scopeLabel")}>
        <span className={styles.scopeLabel}>{t("access.scopeLabel")}</span>
        <div className={styles.segmented}>
          {scopes.map((s) => {
            const disabled = s !== "all" && !hasDevice;
            const label =
              s === "device"
                ? t("access.scope.device")
                : s === "group"
                  ? t("access.scope.group")
                  : t("access.scope.all");
            return (
              <button
                key={s}
                type="button"
                className={styles.segment}
                data-active={scope === s || undefined}
                aria-pressed={scope === s}
                disabled={disabled || busy}
                onClick={() => setScope(s)}
              >
                {label}
              </button>
            );
          })}
        </div>
      </div>
      <p className={styles.scopeHint}>{scopeHint}</p>

      {error && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}

      <div className={styles.actions}>
        <button type="button" className={styles.reject} disabled={busy} onClick={() => void run(() => api.rejectAccessRequest(req.id))}>
          {t("access.reject")}
        </button>
        <button type="button" className={styles.release} disabled={busy} onClick={release}>
          {busy ? t("access.releasing") : t("access.release")}
        </button>
      </div>

      {confirmAll && (
        <ConfirmDialog
          title={t("access.allConfirmTitle")}
          body={t("access.allConfirmBody", { domain: req.domain })}
          confirmLabel={t("access.allConfirmAction")}
          cancelLabel={t("certs.cancel")}
          busy={busy}
          onCancel={() => setConfirmAll(false)}
          onConfirm={() => {
            setConfirmAll(false);
            void run(() => api.releaseAccessRequest(req.id, "all"));
          }}
        />
      )}
    </article>
  );
}

export function AccessRequests() {
  const { t } = useI18n();
  const [requests, setRequests] = useState<AccessRequest[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    api
      .accessRequests(controller.signal)
      .then((list) => setRequests(list))
      .catch((e) => {
        if (controller.signal.aborted) return;
        setError(e instanceof ApiError ? e.message : t("access.loadError"));
        setRequests([]);
      });
    return () => controller.abort();
  }, [t]);

  // Drop a handled card from the queue without a full refetch.
  const onDone = (id: string) =>
    setRequests((prev) => (prev ? prev.filter((r) => r.id !== id) : prev));

  const body = useMemo(() => {
    if (requests === null) return null;
    if (requests.length === 0) {
      return (
        <div className={styles.empty}>
          <p className={styles.emptyTitle}>{t("access.empty")}</p>
          <p className={styles.emptyHint}>{t("access.emptyHint")}</p>
        </div>
      );
    }
    return (
      <div className={styles.grid}>
        {requests.map((req) => (
          <RequestCard key={req.id} req={req} onDone={onDone} />
        ))}
      </div>
    );
  }, [requests, t]);

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>{t("access.title")}</h1>
        <p className={styles.intro}>{t("access.intro")}</p>
      </header>

      {error && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}
      {body}
    </div>
  );
}
