import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type ReactNode,
} from "react";
import { NavLink } from "react-router-dom";

import { ConfirmDialog } from "../components/ConfirmDialog";
import { InfoHint } from "../components/InfoHint";
import { Toggle } from "../components/Toggle";
import { ApiError, api, type CertListItem, type TlsSettings, type TlsStatus } from "../lib/api";
import { bytesToBase64, looksEncrypted } from "../lib/backupFile";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import {
  useSettings,
  type BlockPageSettings,
  type BlockResponse,
  type CustomTransport,
  type CustomUpstream,
  type NotificationSettings,
  type SpeedProfile,
  type UpstreamMode,
  type WebhookFormat,
} from "../state/settings";
import styles from "./Settings.module.css";

/** A short list of common IANA time zones offered as datalist suggestions for
 *  the schedule time zone. Any valid IANA name can still be typed; an unknown
 *  one falls back to UTC on the server. */
const COMMON_TIMEZONES = [
  "UTC",
  "Europe/Vienna",
  "Europe/Berlin",
  "Europe/Zurich",
  "Europe/London",
  "Europe/Paris",
  "Europe/Madrid",
  "Europe/Rome",
  "Europe/Amsterdam",
  "Europe/Warsaw",
  "Europe/Athens",
  "Europe/Istanbul",
  "Europe/Moscow",
  "America/New_York",
  "America/Chicago",
  "America/Denver",
  "America/Los_Angeles",
  "America/Sao_Paulo",
  "Asia/Dubai",
  "Asia/Kolkata",
  "Asia/Singapore",
  "Asia/Shanghai",
  "Asia/Tokyo",
  "Australia/Sydney",
];

/** The "clear DNS cache" action: a one-shot button that drops every cached
 *  answer, with a short confirmation once it is done. */
function ClearCacheButton() {
  const { t } = useI18n();
  const [state, setState] = useState<"idle" | "busy" | "done" | "error">("idle");

  async function clear() {
    setState("busy");
    try {
      await api.clearCache();
      setState("done");
      window.setTimeout(() => setState("idle"), 2500);
    } catch {
      setState("error");
    }
  }

  return (
    <div className={styles.action}>
      <button
        type="button"
        className={styles.actionButton}
        onClick={clear}
        disabled={state === "busy"}
      >
        {state === "busy" ? t("settings.clearCacheBusy") : t("settings.clearCacheButton")}
      </button>
      {state === "done" ? (
        <span className={styles.actionDone}>{t("settings.clearCacheDone")}</span>
      ) : null}
      {state === "error" ? (
        <span className={styles.actionError}>{t("settings.clearCacheError")}</span>
      ) : null}
    </div>
  );
}

/** A small outline padlock, matching the inline-SVG icon style used across the
 *  shell. Inherits its colour from the parent via `currentColor`. */
function LockIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      width="15"
      height="15"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="4" y="10" width="16" height="11" rx="2" />
      <path d="M8 10V7a4 4 0 0 1 8 0v3" />
    </svg>
  );
}

/** Export the whole configuration to a file, and restore one. Export encrypts
 *  the backup with a password the operator types twice and downloads it; restore
 *  reads a chosen file, asks for its password when it is encrypted, confirms,
 *  applies it on the server and reloads so every view reflects the restored data. */
function BackupSection() {
  const { t } = useI18n();
  const fileRef = useRef<HTMLInputElement>(null);
  const [exportState, setExportState] = useState<"idle" | "busy" | "error">("idle");
  const [exportPw, setExportPw] = useState("");
  const [exportPw2, setExportPw2] = useState("");
  const [importState, setImportState] = useState<"idle" | "busy" | "done" | "error">("idle");
  const [importError, setImportError] = useState<string | null>(null);
  const [restorePw, setRestorePw] = useState("");
  // The base64 backup file awaiting the confirm step (with whether it is
  // encrypted), or null when none is pending.
  const [pending, setPending] = useState<{ file: string; encrypted: boolean } | null>(null);

  const mismatch = exportPw !== "" && exportPw2 !== "" && exportPw !== exportPw2;
  const canExport = exportPw !== "" && exportPw === exportPw2 && exportState !== "busy";

  async function download() {
    if (!canExport) return;
    setExportState("busy");
    try {
      const blob = await api.downloadBackup(exportPw);
      const url = URL.createObjectURL(blob);
      const stamp = new Date().toISOString().slice(0, 10);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `justdns-backup-${stamp}.jdbak`;
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      URL.revokeObjectURL(url);
      setExportState("idle");
      setExportPw("");
      setExportPw2("");
    } catch {
      setExportState("error");
    }
  }

  async function onFile(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    // Clear the input so the same file can be picked again after a cancel.
    event.target.value = "";
    if (!file) return;
    setImportState("idle");
    setImportError(null);
    setRestorePw("");
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      setPending({ file: bytesToBase64(bytes), encrypted: looksEncrypted(bytes) });
    } catch {
      setImportState("error");
      setImportError(t("settings.backupImportInvalid"));
    }
  }

  async function confirmRestore() {
    if (!pending) return;
    setImportState("busy");
    try {
      await api.restoreBackup(pending.file, pending.encrypted ? restorePw : undefined);
      setPending(null);
      setRestorePw("");
      setImportState("done");
      // Reload so settings, lists and rewrites views all show the restored data.
      window.setTimeout(() => window.location.reload(), 1200);
    } catch (error) {
      setPending(null);
      setRestorePw("");
      setImportState("error");
      setImportError(error instanceof ApiError ? error.message : t("settings.backupImportError"));
    }
  }

  return (
    <>
      <Row
        label={t("settings.backupExportLabel")}
        description={t("settings.backupExportDesc")}
        details={t("settings.backupExportDetails")}
        control={
          <div className={styles.pwField}>
            <span className={styles.pwLabel}>
              <LockIcon />
              {t("settings.backupExportPassword")}
            </span>
            <input
              type="password"
              className={styles.customInput}
              value={exportPw}
              autoComplete="new-password"
              placeholder={t("settings.backupExportPasswordPlaceholder")}
              aria-label={t("settings.backupExportPassword")}
              onChange={(event) => setExportPw(event.target.value)}
            />
            <input
              type="password"
              className={styles.customInput}
              value={exportPw2}
              autoComplete="new-password"
              placeholder={t("settings.backupExportPasswordConfirmPlaceholder")}
              aria-label={t("settings.backupExportPasswordConfirm")}
              onChange={(event) => setExportPw2(event.target.value)}
            />
            {mismatch ? (
              <span className={styles.actionError}>
                {t("settings.backupExportPasswordMismatch")}
              </span>
            ) : (
              <span className={styles.pwHint}>{t("settings.backupExportPasswordHint")}</span>
            )}
            <div className={styles.action}>
              <button
                type="button"
                className={styles.actionButton}
                onClick={download}
                disabled={!canExport}
              >
                {exportState === "busy"
                  ? t("settings.backupExportBusy")
                  : t("settings.backupExportButton")}
              </button>
              {exportState === "error" ? (
                <span className={styles.actionError}>{t("settings.backupExportError")}</span>
              ) : null}
            </div>
          </div>
        }
      />
      <Row
        label={t("settings.backupImportLabel")}
        description={t("settings.backupImportDesc")}
        details={t("settings.backupImportDetails")}
        control={
          <div className={styles.action}>
            <input
              ref={fileRef}
              type="file"
              accept=".jdbak,application/json,.json,application/octet-stream"
              className={styles.fileInput}
              onChange={onFile}
            />
            <button
              type="button"
              className={styles.actionButton}
              onClick={() => fileRef.current?.click()}
              disabled={importState === "busy"}
            >
              {importState === "busy"
                ? t("settings.backupImportBusy")
                : t("settings.backupImportButton")}
            </button>
            {importState === "done" ? (
              <span className={styles.actionDone}>{t("settings.backupImportDone")}</span>
            ) : null}
            {importState === "error" ? (
              <span className={styles.actionError}>
                {importError ?? t("settings.backupImportError")}
              </span>
            ) : null}
          </div>
        }
      />
      {pending !== null ? (
        <ConfirmDialog
          title={t("settings.backupRestoreConfirmTitle")}
          body={t("settings.backupRestoreConfirmBody")}
          details={
            pending.encrypted ? (
              <div className={styles.encField}>
                <input
                  type="password"
                  className={styles.customInput}
                  value={restorePw}
                  autoComplete="off"
                  placeholder={t("settings.backupRestorePasswordPlaceholder")}
                  aria-label={t("settings.backupRestorePassword")}
                  onChange={(event) => setRestorePw(event.target.value)}
                />
              </div>
            ) : undefined
          }
          confirmLabel={t("settings.backupRestoreConfirm")}
          cancelLabel={t("settings.backupRestoreCancel")}
          busy={importState === "busy"}
          onConfirm={confirmRestore}
          onCancel={() => {
            setPending(null);
            setRestorePw("");
          }}
        />
      ) : null}
    </>
  );
}

interface CategoryMeta {
  id: string;
  titleKey: MessageKey;
}

function Category({
  id,
  title,
  description,
  children,
}: {
  id: string;
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <section className={styles.category} aria-labelledby={`${id}-title`}>
      <div className={styles.categoryHead}>
        <h2 id={`${id}-title`} className={styles.categoryTitle}>
          {title}
        </h2>
        <p className={styles.categoryDescription}>{description}</p>
      </div>
      <div className={styles.rows}>{children}</div>
    </section>
  );
}

function Row({
  label,
  description,
  details,
  control,
  disabled = false,
}: {
  label: string;
  description?: string;
  details?: string;
  control: ReactNode;
  disabled?: boolean;
}) {
  return (
    <div className={styles.row} data-disabled={disabled || undefined}>
      <div className={styles.text}>
        <span className={styles.labelLine}>
          <span className={styles.label}>{label}</span>
          {details ? <InfoHint label={label} text={details} /> : null}
        </span>
        {description ? <span className={styles.description}>{description}</span> : null}
      </div>
      <div className={styles.control}>{control}</div>
    </div>
  );
}

/** The upstream selection modes, in the order the dropdown lists them. Each maps
 *  to a `settings.mode.<mode>` label key. */
const UPSTREAM_MODES: UpstreamMode[] = ["racing", "fallback", "balanced", "rules"];

/** The speed profiles, in the order the switch lists them: most private first,
 *  fastest last. Each maps to `settings.profile.<profile>` label and
 *  `settings.profile.<profile>Desc` description keys. */
const PROFILES: SpeedProfile[] = ["privacy", "balanced", "speed"];

/** The resolvers each profile uses. Mirrors `profile_resolvers` in the Rust
 *  binary, so the list shown here matches what actually answers queries. */
const PROFILE_RESOLVERS: Record<SpeedProfile, { ip: string; host: string }[]> = {
  privacy: [
    { ip: "86.54.11.1", host: "protective.joindns4.eu" },
    { ip: "9.9.9.9", host: "dns.quad9.net" },
  ],
  balanced: [
    { ip: "86.54.11.1", host: "protective.joindns4.eu" },
    { ip: "1.1.1.1", host: "cloudflare-dns.com" },
  ],
  speed: [
    { ip: "1.1.1.1", host: "cloudflare-dns.com" },
    { ip: "8.8.8.8", host: "dns.google" },
  ],
};

/** The transports a custom resolver can use, in dropdown order. */
const TRANSPORTS: CustomTransport[] = ["plain", "dot", "doh"];

/** A permissive check that a string looks like an IPv4 or IPv6 literal — enough
 *  to flag an obviously wrong entry in the UI; the server validates for real. */
function looksLikeIp(value: string): boolean {
  const v = value.trim();
  if (v === "") return false;
  if (v.includes(":")) return /^[0-9a-fA-F:]+$/.test(v) && (v.match(/:/g)?.length ?? 0) >= 2;
  const parts = v.split(".");
  return parts.length === 4 && parts.every((p) => /^\d{1,3}$/.test(p) && Number(p) <= 255);
}

/** A custom resolver is usable once its address parses and — for the encrypted
 *  transports — it carries a certificate name. */
function customUpstreamValid(entry: CustomUpstream): boolean {
  if (!looksLikeIp(entry.address)) return false;
  if (entry.transport !== "plain" && !(entry.serverName ?? "").trim()) return false;
  return true;
}

/** Editor for the user's own resolver list, shown when "use my own resolvers"
 *  is on. Each row is an address, a transport, and — for DoT/DoH — a certificate
 *  name; invalid rows are flagged here and skipped by the server. */
function CustomUpstreamsEditor() {
  const { t } = useI18n();
  const { settings, update } = useSettings();
  const entries = settings.customUpstreams;

  const setEntry = (index: number, patch: Partial<CustomUpstream>) =>
    update(
      "customUpstreams",
      entries.map((entry, i) => (i === index ? { ...entry, ...patch } : entry)),
    );
  const addEntry = () =>
    update("customUpstreams", [
      ...entries,
      { address: "", port: null, transport: "plain", serverName: null },
    ]);
  const removeEntry = (index: number) =>
    update(
      "customUpstreams",
      entries.filter((_, i) => i !== index),
    );

  const anyValid = entries.some(customUpstreamValid);

  return (
    <div className={styles.custom}>
      {entries.length === 0 ? (
        <p className={styles.customEmpty}>{t("settings.customEmpty")}</p>
      ) : (
        <ul className={styles.customList}>
          {entries.map((entry, index) => {
            const needsName = entry.transport !== "plain";
            const invalid = !customUpstreamValid(entry);
            return (
              <li key={index} className={styles.customRow} data-invalid={invalid || undefined}>
                <input
                  className={styles.customInput}
                  value={entry.address}
                  onChange={(e) => setEntry(index, { address: e.target.value })}
                  placeholder={t("settings.customAddrPlaceholder")}
                  aria-label={t("settings.customAddrLabel")}
                  inputMode="numeric"
                />
                <select
                  className={styles.select}
                  value={entry.transport}
                  aria-label={t("settings.customTransportLabel")}
                  onChange={(e) =>
                    setEntry(index, { transport: e.target.value as CustomTransport })
                  }
                >
                  {TRANSPORTS.map((transport) => (
                    <option key={transport} value={transport}>
                      {t(`settings.transport.${transport}` as MessageKey)}
                    </option>
                  ))}
                </select>
                <input
                  className={styles.customInput}
                  value={entry.serverName ?? ""}
                  onChange={(e) => setEntry(index, { serverName: e.target.value || null })}
                  placeholder={t("settings.customNamePlaceholder")}
                  aria-label={t("settings.customNameLabel")}
                  disabled={!needsName}
                  data-hidden={!needsName || undefined}
                />
                <button
                  type="button"
                  className={styles.customRemove}
                  onClick={() => removeEntry(index)}
                  aria-label={t("settings.customRemove")}
                  title={t("settings.customRemove")}
                >
                  ✕
                </button>
              </li>
            );
          })}
        </ul>
      )}
      <button type="button" className={styles.actionButton} onClick={addEntry}>
        {t("settings.customAdd")}
      </button>
      {entries.length > 0 && !anyValid ? (
        <p className={styles.customWarn}>{t("settings.customNoneValid")}</p>
      ) : null}
    </div>
  );
}

/** The block-response control: a segmented switch between answering a blocked
 *  name with the null IP (0.0.0.0 / ::) and with NXDOMAIN. */
const BLOCK_RESPONSES: BlockResponse[] = ["null-ip", "nx-domain"];

function BlockResponseControl() {
  const { t } = useI18n();
  const { settings, update } = useSettings();
  const current = settings.blockResponse;

  return (
    <div className={styles.profile}>
      <span className={styles.labelLine}>
        <span className={styles.label}>{t("settings.blockResponseLabel")}</span>
        <InfoHint
          label={t("settings.blockResponseLabel")}
          text={t("settings.blockResponseDetails")}
        />
      </span>
      <div
        className={styles.profileSwitch}
        role="group"
        aria-label={t("settings.blockResponseLabel")}
      >
        {BLOCK_RESPONSES.map((opt) => (
          <button
            key={opt}
            type="button"
            className={styles.profileButton}
            data-active={current === opt || undefined}
            aria-pressed={current === opt}
            onClick={() => update("blockResponse", opt)}
          >
            {t(`settings.blockResponse.${opt}` as MessageKey)}
          </button>
        ))}
      </div>
      <p className={styles.profileDesc}>
        {t(`settings.blockResponse.${current}Desc` as MessageKey)}
      </p>
    </div>
  );
}

/** Load the certificate catalogue once, for the DoT/DoH and block-page
 *  selectors. Returns `null` while loading, `[]` when the store is unavailable
 *  (so the caller can show a notice), or the list. */
function useCertList(): CertListItem[] | null {
  const [certs, setCerts] = useState<CertListItem[] | null>(null);
  useEffect(() => {
    let cancelled = false;
    api
      .certs()
      .then((list) => !cancelled && setCerts(list))
      .catch(() => !cancelled && setCerts([]));
    return () => {
      cancelled = true;
    };
  }, []);
  return certs;
}

/** A dropdown over the stored certificates, with a leading "store default"
 *  option (empty value). Expired certificates are flagged in their label. */
function CertSelect({
  value,
  certs,
  ariaLabel,
  onChange,
}: {
  value: string;
  certs: CertListItem[];
  ariaLabel: string;
  onChange: (id: string) => void;
}) {
  const { t } = useI18n();
  return (
    <select
      className={styles.select}
      aria-label={ariaLabel}
      value={value}
      onChange={(e) => onChange(e.target.value)}
    >
      <option value="">{t("settings.encDns.certDefaultOption")}</option>
      {certs.map((cert) => (
        <option key={cert.id} value={cert.id}>
          {cert.expired ? `${cert.name} (${t("certs.badgeExpired")})` : cert.name}
        </option>
      ))}
    </select>
  );
}

/** A copy-to-clipboard button for an endpoint address. */
function CopyButton({ value }: { value: string }) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className={styles.copyButton}
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
          window.setTimeout(() => setCopied(false), 1500);
        } catch {
          // Clipboard blocked (e.g. insecure context): the value is still shown
          // for the user to select and copy by hand.
        }
      }}
    >
      {copied ? t("settings.encDns.copied") : t("settings.encDns.copy")}
    </button>
  );
}

/** The encrypted client-DNS section: DoT/DoH toggles, the hostname and
 *  certificate source, the CA download or certificate upload, the fingerprint,
 *  and copy-ready endpoint addresses. Backed by the `/api/tls` endpoints rather
 *  than the shared settings store, so it fetches and saves its own state. */
function EncryptedDnsSection() {
  const { t } = useI18n();
  const [status, setStatus] = useState<TlsStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hostnameDraft, setHostnameDraft] = useState("");
  const certs = useCertList();
  // Set when the user tries to switch an endpoint on without a hostname: the
  // field is highlighted and focused, and the switch stays off until one is set.
  const [needsHostname, setNeedsHostname] = useState(false);
  const hostnameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .tls()
      .then((loaded) => {
        if (cancelled) return;
        setStatus(loaded);
        setHostnameDraft(loaded.hostname);
      })
      .catch(() => {
        if (!cancelled) setError(t("settings.encDns.saveError"));
      });
    return () => {
      cancelled = true;
    };
    // Loaded once on mount; `t` is stable enough that re-fetching on a locale
    // change is not worth the extra request.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function patch(partial: Partial<TlsSettings>) {
    if (!status) return;
    const next: TlsSettings = {
      dotEnabled: status.dotEnabled,
      dohEnabled: status.dohEnabled,
      hostname: status.hostname,
      dotCertId: status.dotCertId,
      dohCertId: status.dohCertId,
      ...partial,
    };
    setError(null);
    try {
      const updated = await api.saveTls(next);
      setStatus(updated);
      setHostnameDraft(updated.hostname);
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("settings.encDns.saveError"));
    }
  }

  // Enabling DoT/DoH needs a hostname: without one we refuse the switch, ask for
  // a hostname and focus the field, so the endpoint never comes up without a
  // name in its certificate. Turning off is always allowed.
  function toggleFeature(field: "dotEnabled" | "dohEnabled", on: boolean) {
    if (!status) return;
    if (!on) {
      setNeedsHostname(false);
      patch(field === "dotEnabled" ? { dotEnabled: false } : { dohEnabled: false });
      return;
    }
    const effectiveHostname = hostnameDraft.trim() || status.hostname.trim();
    if (!effectiveHostname) {
      setNeedsHostname(true);
      hostnameRef.current?.focus();
      return;
    }
    setNeedsHostname(false);
    const partial: Partial<TlsSettings> = { hostname: effectiveHostname };
    if (field === "dotEnabled") partial.dotEnabled = true;
    else partial.dohEnabled = true;
    patch(partial);
  }

  if (!status) {
    return error ? <p className={styles.encUnavailable}>{error}</p> : null;
  }
  if (!status.available) {
    return <p className={styles.encUnavailable}>{t("settings.encDns.unavailable")}</p>;
  }

  const hostname = status.hostname.trim();
  const featureActive = status.dotEnabled || status.dohEnabled;
  const dotEndpoint = hostname ? `${hostname}:${status.dotPort}` : "";
  const dohEndpoint = hostname ? `https://${hostname}:${status.dohPort}/dns-query` : "";

  return (
    <>
      {/* Hostname first: the certificate and the endpoint addresses both build
          on it, so nothing below works until it is set. */}
      <Row
        label={t("settings.encDns.hostnameLabel")}
        description={t("settings.encDns.hostnameDesc")}
        details={t("settings.encDns.hostnameDetails")}
        control={
          <div className={styles.hostnameControl}>
            <input
              ref={hostnameRef}
              className={styles.select}
              type="text"
              value={hostnameDraft}
              placeholder={t("settings.encDns.hostnamePlaceholder")}
              aria-label={t("settings.encDns.hostnameLabel")}
              data-required={needsHostname || undefined}
              onChange={(e) => {
                setHostnameDraft(e.target.value);
                if (e.target.value.trim()) setNeedsHostname(false);
              }}
              onBlur={() => {
                const draft = hostnameDraft.trim();
                // Never persist an empty hostname while an endpoint is live.
                if (!draft && featureActive) {
                  setHostnameDraft(status.hostname);
                  return;
                }
                if (draft) setNeedsHostname(false);
                if (draft !== status.hostname) patch({ hostname: draft });
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") e.currentTarget.blur();
              }}
            />
          </div>
        }
      />

      {needsHostname ? (
        <p className={styles.encRequired}>{t("settings.encDns.hostnameRequired")}</p>
      ) : null}

      {/* The two endpoint switches. Turning one on without a hostname is refused
          by toggleFeature, which asks for one instead of enabling. */}
      <Row
        label={t("settings.encDns.dotLabel")}
        description={t("settings.encDns.dotDesc")}
        details={t("settings.encDns.dotDetails")}
        control={
          <Toggle
            label={t("settings.encDns.dotLabel")}
            checked={status.dotEnabled}
            onChange={(v) => toggleFeature("dotEnabled", v)}
          />
        }
      />
      <Row
        label={t("settings.encDns.dohLabel")}
        description={t("settings.encDns.dohDesc")}
        details={t("settings.encDns.dohDetails")}
        control={
          <Toggle
            label={t("settings.encDns.dohLabel")}
            checked={status.dohEnabled}
            onChange={(v) => toggleFeature("dohEnabled", v)}
          />
        }
      />

      {/* Certificate selection and endpoint addresses only make sense once an
          endpoint is actually running — hidden until then to keep the page
          focused. The certificates themselves are managed on the Certificates
          page; here each endpoint just picks one. */}
      {featureActive ? (
        <>
      <Row
        label={t("settings.encDns.dotCertLabel")}
        description={t("settings.encDns.certSelectDetails")}
        control={
          certs ? (
            <CertSelect
              value={status.dotCertId}
              certs={certs}
              ariaLabel={t("settings.encDns.dotCertLabel")}
              onChange={(id) => patch({ dotCertId: id })}
            />
          ) : null
        }
      />
      <Row
        label={t("settings.encDns.dohCertLabel")}
        control={
          certs ? (
            <CertSelect
              value={status.dohCertId}
              certs={certs}
              ariaLabel={t("settings.encDns.dohCertLabel")}
              onChange={(id) => patch({ dohCertId: id })}
            />
          ) : null
        }
      />
      <p className={styles.encHint}>
        <NavLink to="/zertifikate" className={styles.inlineLink}>
          {t("settings.encDns.certManageLink")}
        </NavLink>
      </p>

      {hostname ? (
        <div className={styles.encEndpoints}>
          <span className={styles.resolversTitle}>{t("settings.encDns.endpointsTitle")}</span>
          <div className={styles.encEndpointRow}>
            <span className={styles.encEndpointLabel}>{t("settings.encDns.dotEndpointLabel")}</span>
            <code className={styles.encCode}>{dotEndpoint}</code>
            <CopyButton value={dotEndpoint} />
          </div>
          <div className={styles.encEndpointRow}>
            <span className={styles.encEndpointLabel}>{t("settings.encDns.dohEndpointLabel")}</span>
            <code className={styles.encCode}>{dohEndpoint}</code>
            <CopyButton value={dohEndpoint} />
          </div>
        </div>
      ) : null}
        </>
      ) : null}

      {error ? <p className={styles.actionError}>{error}</p> : null}
    </>
  );
}

/** The host a landing URL points at, for the setup hints. Empty when the URL is
 *  blank or unparseable. Accepts a URL with or without a scheme. */
function landingHostOf(url: string): string {
  const raw = url.trim();
  if (raw === "") return "";
  try {
    return new URL(raw.includes("://") ? raw : `https://${raw}`).host;
  } catch {
    return "";
  }
}

/** The custom block-page section. A single mode switch drives everything:
 *  "hosted" redirects blocked domains to the operator's own hostname (a valid
 *  certificate, no warning), "local" serves the built-in page on this box. The
 *  server IP (where blocked names point) is the one field both modes need. This
 *  section reads and writes through the shared `useSettings` store. */
function BlockPageSection() {
  const { t } = useI18n();
  const { settings, update } = useSettings();
  const certs = useCertList();
  const bp = settings.blockPage;

  // A successful fetch always returns at least the store default, so an empty
  // list means the certificate store could not be prepared.
  const storeUnavailable = certs !== null && certs.length === 0;

  // The mode is derived from the saved settings: the reverse-proxy flag wins,
  // then a landing URL means hosted, otherwise local. A draft keeps the typed
  // URL while the user is in local mode, so switching back restores it.
  type BpMode = "hosted" | "proxy" | "local";
  const [mode, setMode] = useState<BpMode>(
    bp.behindReverseProxy ? "proxy" : (bp.redirectUrl ?? "") !== "" ? "hosted" : "local",
  );
  const [draftUrl, setDraftUrl] = useState(bp.redirectUrl ?? "");

  function set<K extends keyof BlockPageSettings>(key: K, value: BlockPageSettings[K]) {
    update("blockPage", { ...bp, [key]: value });
  }
  function chooseMode(next: BpMode) {
    setMode(next);
    // Both hosted and proxy use a landing URL; only proxy trusts the forwarded
    // host and lets the proxy terminate TLS. Local serves the page in place.
    update("blockPage", {
      ...bp,
      redirectUrl: next === "local" ? "" : draftUrl,
      behindReverseProxy: next === "proxy",
    });
  }
  function changeUrl(value: string) {
    setDraftUrl(value);
    set("redirectUrl", value);
  }

  const host = landingHostOf(draftUrl);
  const ipMissing = bp.serverIpv4.trim() === "";
  const ipTarget = bp.serverIpv4.trim() || t("settings.blockPage.yourNodeIp");

  const ipv4Row = (
    <Row
      label={t("settings.blockPage.ipv4Label")}
      description={t("settings.blockPage.ipv4Desc")}
      details={t("settings.blockPage.ipv4Details")}
      control={
        <input
          className={styles.select}
          type="text"
          inputMode="numeric"
          aria-label={t("settings.blockPage.ipv4Label")}
          value={bp.serverIpv4}
          placeholder="192.168.20.2"
          spellCheck={false}
          onChange={(e) => set("serverIpv4", e.target.value.trim())}
        />
      }
    />
  );
  const ipv6Row = (
    <Row
      label={t("settings.blockPage.ipv6Label")}
      description={t("settings.blockPage.ipv6Desc")}
      details={t("settings.blockPage.ipv6Details")}
      control={
        <input
          className={styles.select}
          type="text"
          aria-label={t("settings.blockPage.ipv6Label")}
          value={bp.serverIpv6}
          placeholder="fd00::2"
          spellCheck={false}
          onChange={(e) => set("serverIpv6", e.target.value.trim())}
        />
      }
    />
  );
  const titleRow = (
    <Row
      label={t("settings.blockPage.titleLabel")}
      description={t("settings.blockPage.titleDesc")}
      control={
        <input
          className={styles.select}
          type="text"
          aria-label={t("settings.blockPage.titleLabel")}
          value={bp.title}
          onChange={(e) => set("title", e.target.value)}
        />
      }
    />
  );
  const messageRow = (
    <Row
      label={t("settings.blockPage.messageLabel")}
      description={t("settings.blockPage.messageDesc")}
      control={
        <textarea
          className={styles.blockPageText}
          aria-label={t("settings.blockPage.messageLabel")}
          value={bp.message}
          rows={3}
          onChange={(e) => set("message", e.target.value)}
        />
      }
    />
  );

  return (
    <>
      <Row
        label={t("settings.blockPage.enabledLabel")}
        description={t("settings.blockPage.enabledDesc")}
        details={t("settings.blockPage.enabledDetails")}
        control={
          <Toggle
            label={t("settings.blockPage.enabledLabel")}
            checked={bp.enabled}
            onChange={(v) => set("enabled", v)}
          />
        }
      />

      {bp.enabled ? (
        storeUnavailable ? (
          <p className={styles.encUnavailable}>{t("settings.blockPage.unavailable")}</p>
        ) : (
          <>
            <Row
              label={t("settings.blockPage.modeLabel")}
              description={t("settings.blockPage.modeDesc")}
              control={
                <select
                  className={styles.select}
                  aria-label={t("settings.blockPage.modeLabel")}
                  value={mode}
                  onChange={(e) => chooseMode(e.target.value as BpMode)}
                >
                  <option value="proxy">{t("settings.blockPage.modeProxy")}</option>
                  <option value="hosted">{t("settings.blockPage.modeHosted")}</option>
                  <option value="local">{t("settings.blockPage.modeLocal")}</option>
                </select>
              }
            />

            {mode === "hosted" ? (
              <>
                <Row
                  label={t("settings.blockPage.urlLabel")}
                  description={t("settings.blockPage.urlDesc")}
                  details={t("settings.blockPage.urlDetails")}
                  control={
                    <input
                      className={styles.select}
                      type="text"
                      inputMode="url"
                      aria-label={t("settings.blockPage.urlLabel")}
                      value={draftUrl}
                      placeholder="https://justdns.justfuture.at/blockpage"
                      spellCheck={false}
                      onChange={(e) => changeUrl(e.target.value.trim())}
                    />
                  }
                />
                {ipv4Row}
                <Row
                  label={t("settings.blockPage.certLabel")}
                  description={t("settings.blockPage.certDesc")}
                  details={t("settings.blockPage.certHostedDetails")}
                  control={
                    certs ? (
                      <CertSelect
                        value={bp.certId ?? ""}
                        certs={certs}
                        ariaLabel={t("settings.blockPage.certLabel")}
                        onChange={(id) => set("certId", id === "" ? null : id)}
                      />
                    ) : null
                  }
                />
                {host !== "" && (
                  <div className={styles.encHint}>
                    <p>
                      1. {t("settings.blockPage.hostedRewriteHint")}{" "}
                      <code>
                        {host} → {ipTarget}
                      </code>
                    </p>
                    <p>
                      2. {t("settings.blockPage.hostedCertHint")} <code>{host}</code>
                    </p>
                  </div>
                )}
                {titleRow}
                {messageRow}
                <p className={styles.encHint}>{t("settings.blockPage.hostedHttpsNote")}</p>
              </>
            ) : mode === "proxy" ? (
              <>
                <Row
                  label={t("settings.blockPage.urlLabel")}
                  description={t("settings.blockPage.proxyUrlDesc")}
                  details={t("settings.blockPage.proxyUrlDetails")}
                  control={
                    <input
                      className={styles.select}
                      type="text"
                      inputMode="url"
                      aria-label={t("settings.blockPage.urlLabel")}
                      value={draftUrl}
                      placeholder="https://justdns.justfuture.at/blockpage"
                      spellCheck={false}
                      onChange={(e) => changeUrl(e.target.value.trim())}
                    />
                  }
                />
                {ipv4Row}
                <p className={styles.encHint}>{t("settings.blockPage.proxyNodeNote")}</p>
                {host !== "" && (
                  <div className={styles.encHint}>
                    <p>{t("settings.blockPage.proxyNpmIntro")}</p>
                    <ul>
                      <li>
                        {t("settings.blockPage.proxyNpmDomain")} <code>{host}</code>
                      </li>
                      <li>
                        {t("settings.blockPage.proxyNpmForward")}{" "}
                        <code>
                          http://{ipTarget}:80
                        </code>
                      </li>
                      <li>{t("settings.blockPage.proxyNpmHeader")}</li>
                      <li>{t("settings.blockPage.proxyNpmSsl")}</li>
                    </ul>
                  </div>
                )}
                {titleRow}
                {messageRow}
                <p className={styles.encHint}>{t("settings.blockPage.proxyHttpsNote")}</p>
              </>
            ) : (
              <>
                {ipv4Row}
                <Row
                  label={t("settings.blockPage.certLabel")}
                  description={t("settings.blockPage.certDesc")}
                  details={t("settings.blockPage.certLocalDetails")}
                  control={
                    certs ? (
                      <CertSelect
                        value={bp.certId ?? ""}
                        certs={certs}
                        ariaLabel={t("settings.blockPage.certLabel")}
                        onChange={(id) => set("certId", id === "" ? null : id)}
                      />
                    ) : null
                  }
                />
                {titleRow}
                {messageRow}
                <p className={styles.encHint}>{t("settings.blockPage.localHttpsNote")}</p>
              </>
            )}

            {ipv6Row}
            {ipMissing && (
              <p className={styles.encUnavailable}>{t("settings.blockPage.ipRequired")}</p>
            )}
            <p className={styles.encHint}>
              {t(mode === "proxy" ? "settings.blockPage.proxyPortsHint" : "settings.blockPage.portsHint")}
            </p>
          </>
        )
      ) : null}
    </>
  );
}

/** The webhook shapes offered in the format dropdown, in listed order. Each maps
 *  to a `settings.notifications.format.<format>` label key. `generic` sends a
 *  plain `{title, message}` JSON body that ntfy and most receivers accept. */
const WEBHOOK_FORMATS: WebhookFormat[] = ["generic", "ntfy", "slack", "discord", "telegram"];

function NotificationsSection() {
  const { t } = useI18n();
  const { settings, update } = useSettings();
  const n = settings.notifications;
  const [testState, setTestState] = useState<"idle" | "busy" | "done" | "error">("idle");
  const [testError, setTestError] = useState<string | null>(null);

  function set<K extends keyof NotificationSettings>(key: K, value: NotificationSettings[K]) {
    update("notifications", { ...n, [key]: value });
  }

  // The test uses the saved config, so the debounced save must have landed
  // first; the hint under the button spells that out. A webhook error (bad URL,
  // non-2xx from the receiver) comes back as the ApiError detail.
  async function sendTest() {
    setTestState("busy");
    setTestError(null);
    try {
      await api.notificationsTest();
      setTestState("done");
      window.setTimeout(() => setTestState("idle"), 3000);
    } catch (err) {
      setTestError(err instanceof ApiError ? err.message : String(err));
      setTestState("error");
    }
  }

  const isTelegram = n.webhookFormat === "telegram";

  return (
    <>
      <Row
        label={t("settings.notifications.enabledLabel")}
        description={t("settings.notifications.enabledDesc")}
        details={t("settings.notifications.enabledDetails")}
        control={
          <Toggle
            label={t("settings.notifications.enabledLabel")}
            checked={n.enabled}
            onChange={(v) => set("enabled", v)}
          />
        }
      />

      {n.enabled ? (
        <>
          <Row
            label={t("settings.notifications.formatLabel")}
            description={t("settings.notifications.formatDesc")}
            details={t("settings.notifications.formatDetails")}
            control={
              <select
                className={styles.select}
                aria-label={t("settings.notifications.formatLabel")}
                value={n.webhookFormat}
                onChange={(e) => set("webhookFormat", e.target.value as WebhookFormat)}
              >
                {WEBHOOK_FORMATS.map((f) => (
                  <option key={f} value={f}>
                    {t(`settings.notifications.format.${f}` as MessageKey)}
                  </option>
                ))}
              </select>
            }
          />

          <Row
            label={
              isTelegram
                ? t("settings.notifications.tokenLabel")
                : t("settings.notifications.urlLabel")
            }
            description={
              isTelegram
                ? t("settings.notifications.tokenDesc")
                : t("settings.notifications.urlDesc")
            }
            details={
              isTelegram
                ? t("settings.notifications.tokenDetails")
                : t("settings.notifications.urlDetails")
            }
            control={
              <input
                className={styles.select}
                // A webhook URL often carries a secret token; mask it like a
                // password so it is not shoulder-surfed. The value still round-
                // trips to the server unchanged.
                type="password"
                autoComplete="off"
                aria-label={
                  isTelegram
                    ? t("settings.notifications.tokenLabel")
                    : t("settings.notifications.urlLabel")
                }
                value={n.webhookUrl}
                placeholder={isTelegram ? "123456:ABC-DEF..." : "https://ntfy.sh/mein-alarm"}
                spellCheck={false}
                onChange={(e) => set("webhookUrl", e.target.value.trim())}
              />
            }
          />

          {isTelegram ? (
            <Row
              label={t("settings.notifications.chatIdLabel")}
              description={t("settings.notifications.chatIdDesc")}
              control={
                <input
                  className={styles.select}
                  type="text"
                  aria-label={t("settings.notifications.chatIdLabel")}
                  value={n.telegramChatId}
                  placeholder="-1001234567890"
                  spellCheck={false}
                  onChange={(e) => set("telegramChatId", e.target.value.trim())}
                />
              }
            />
          ) : null}

          <Row
            label={t("settings.notifications.onNodeDownLabel")}
            description={t("settings.notifications.onNodeDownDesc")}
            control={
              <Toggle
                label={t("settings.notifications.onNodeDownLabel")}
                checked={n.onNodeDown}
                onChange={(v) => set("onNodeDown", v)}
              />
            }
          />
          <Row
            label={t("settings.notifications.onUpdateLabel")}
            description={t("settings.notifications.onUpdateDesc")}
            control={
              <Toggle
                label={t("settings.notifications.onUpdateLabel")}
                checked={n.onUpdate}
                onChange={(v) => set("onUpdate", v)}
              />
            }
          />
          <Row
            label={t("settings.notifications.onCertExpiryLabel")}
            description={t("settings.notifications.onCertExpiryDesc")}
            control={
              <Toggle
                label={t("settings.notifications.onCertExpiryLabel")}
                checked={n.onCertExpiry}
                onChange={(v) => set("onCertExpiry", v)}
              />
            }
          />
          {n.onCertExpiry ? (
            <Row
              label={t("settings.notifications.certDaysLabel")}
              description={t("settings.notifications.certDaysDesc")}
              control={
                <input
                  className={styles.select}
                  type="number"
                  min={1}
                  max={90}
                  aria-label={t("settings.notifications.certDaysLabel")}
                  value={n.certExpiryDays}
                  onChange={(e) =>
                    set("certExpiryDays", Math.max(1, Math.min(90, Number(e.target.value) || 1)))
                  }
                />
              }
            />
          ) : null}

          <Row
            label={t("settings.notifications.onSpikeLabel")}
            description={t("settings.notifications.onSpikeDesc")}
            details={t("settings.notifications.onSpikeDetails")}
            control={
              <Toggle
                label={t("settings.notifications.onSpikeLabel")}
                checked={n.onDeviceSpike}
                onChange={(v) => set("onDeviceSpike", v)}
              />
            }
          />
          {n.onDeviceSpike ? (
            <>
              <Row
                label={t("settings.notifications.spikeThresholdLabel")}
                description={t("settings.notifications.spikeThresholdDesc")}
                control={
                  <input
                    className={styles.select}
                    type="number"
                    min={1}
                    aria-label={t("settings.notifications.spikeThresholdLabel")}
                    value={n.spikeBlockedThreshold}
                    onChange={(e) =>
                      set("spikeBlockedThreshold", Math.max(1, Number(e.target.value) || 1))
                    }
                  />
                }
              />
              <Row
                label={t("settings.notifications.spikeWindowLabel")}
                description={t("settings.notifications.spikeWindowDesc")}
                control={
                  <input
                    className={styles.select}
                    type="number"
                    min={1}
                    max={1440}
                    aria-label={t("settings.notifications.spikeWindowLabel")}
                    value={n.spikeWindowMins}
                    onChange={(e) =>
                      set("spikeWindowMins", Math.max(1, Math.min(1440, Number(e.target.value) || 1)))
                    }
                  />
                }
              />
            </>
          ) : null}

          <div className={styles.action}>
            <button
              type="button"
              className={styles.actionButton}
              onClick={sendTest}
              disabled={testState === "busy" || n.webhookUrl.trim() === ""}
            >
              {testState === "busy"
                ? t("settings.notifications.testBusy")
                : t("settings.notifications.testButton")}
            </button>
            {testState === "done" ? (
              <span className={styles.actionDone}>{t("settings.notifications.testDone")}</span>
            ) : null}
            {testState === "error" ? (
              <span className={styles.actionError}>
                {t("settings.notifications.testError")}
                {testError ? `: ${testError}` : ""}
              </span>
            ) : null}
          </div>
          <p className={styles.encHint}>{t("settings.notifications.testHint")}</p>
        </>
      ) : null}
    </>
  );
}

/** The Prometheus metrics token. The `/metrics` scrape endpoint is off until a
 *  bearer token is set here; the admin generates one (or types their own), copies
 *  it into their Prometheus scrape config, and can clear it to turn the endpoint
 *  off again. The token is a write-only secret stored in the encrypted config. */
function MetricsSection() {
  const { t } = useI18n();
  const { settings, update } = useSettings();
  const token = settings.metricsToken;
  const enabled = token.trim() !== "";
  const [copied, setCopied] = useState(false);

  function generate() {
    const bytes = new Uint8Array(32);
    crypto.getRandomValues(bytes);
    const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
    update("metricsToken", hex);
    setCopied(false);
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(token);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      // A browser that refuses clipboard access leaves the token on screen to
      // copy by hand; nothing else to do.
    }
  }

  return (
    <Row
      label={t("settings.metrics.label")}
      description={t("settings.metrics.desc")}
      details={t("settings.metrics.details")}
      control={
        <div className={styles.pwField}>
          <span className={styles.pwLabel}>
            <LockIcon />
            {enabled ? t("settings.metrics.on") : t("settings.metrics.off")}
          </span>
          <input
            type="text"
            className={styles.customInput}
            style={{ fontFamily: "var(--font-mono, monospace)" }}
            value={token}
            spellCheck={false}
            autoComplete="off"
            placeholder={t("settings.metrics.placeholder")}
            aria-label={t("settings.metrics.label")}
            onChange={(event) => update("metricsToken", event.target.value.trim())}
          />
          <div className={styles.action}>
            <button type="button" className={styles.actionButton} onClick={generate}>
              {t("settings.metrics.generate")}
            </button>
            {enabled ? (
              <button type="button" className={styles.actionButton} onClick={copy}>
                {copied ? t("settings.metrics.copied") : t("settings.metrics.copy")}
              </button>
            ) : null}
            {enabled ? (
              <button
                type="button"
                className={styles.actionButton}
                onClick={() => update("metricsToken", "")}
              >
                {t("settings.metrics.clear")}
              </button>
            ) : null}
          </div>
          <span className={styles.pwHint}>{t("settings.metrics.pathHint")}</span>
        </div>
      }
    />
  );
}

export function Settings() {
  const { t } = useI18n();
  const { settings, update } = useSettings();
  const [selected, setSelected] = useState("schutz");

  const categories = useMemo<CategoryMeta[]>(() => {
    // "Upstream" now hosts the speed profile — the headline user-facing choice —
    // so it stays visible even outside expert mode; the rest stay technical.
    const base: CategoryMeta[] = [
      { id: "schutz", titleKey: "settings.cat.schutz" },
      { id: "blockseite", titleKey: "settings.cat.blockseite" },
      { id: "upstream", titleKey: "settings.cat.upstream" },
      // Backup & restore stays visible even outside expert mode: making a
      // safety copy is a headline user-facing action, not a technical knob.
      { id: "sicherung", titleKey: "settings.cat.sicherung" },
      // Alerts are an operator-facing headline too: a node going down or a
      // certificate expiring is exactly what someone wants pushed to them, so
      // it stays out from behind expert mode.
      { id: "benachrichtigungen", titleKey: "settings.cat.benachrichtigungen" },
    ];
    if (settings.expertMode) {
      base.push(
        // Encryption (both directions: client DoT/DoH and upstream) is a
        // technical area, so it lives behind expert mode.
        { id: "verschluesselung", titleKey: "settings.cat.verschluesselung" },
        { id: "netzwerk", titleKey: "settings.cat.netzwerk" },
        { id: "cache", titleKey: "settings.cat.cache" },
        { id: "protokoll", titleKey: "settings.cat.protokoll" },
      );
    }
    return base;
  }, [settings.expertMode]);

  // Fall back to the first category when the selected one is hidden (e.g. the
  // expert mode that revealed it was switched off).
  const active = categories.some((c) => c.id === selected) ? selected : categories[0].id;

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>{t("settings.title")}</h1>

      <section className={styles.expertBand}>
        <div className={styles.text}>
          <span className={styles.label}>{t("settings.expertMode")}</span>
          <span className={styles.description}>{t("settings.expertModeDesc")}</span>
        </div>
        <Toggle
          label={t("settings.expertMode")}
          checked={settings.expertMode}
          onChange={(v) => update("expertMode", v)}
        />
      </section>

      <div className={styles.layout}>
        <nav className={styles.rail} aria-label={t("settings.categoriesAria")}>
          {categories.map((category) => (
            <button
              key={category.id}
              type="button"
              className={active === category.id ? styles.railActive : styles.railLink}
              aria-current={active === category.id || undefined}
              onClick={() => setSelected(category.id)}
            >
              {t(category.titleKey)}
            </button>
          ))}
          {!settings.expertMode && <p className={styles.railHint}>{t("settings.railHint")}</p>}
        </nav>

        <div className={styles.sections}>
          {active === "schutz" && (
            <Category
              id="schutz"
              title={t("settings.cat.schutz")}
              description={t("settings.schutzDesc")}
            >
              <Row
                label={t("settings.instanceNameLabel")}
                description={t("settings.instanceNameDesc")}
                details={t("settings.instanceNameDetails")}
                control={
                  <input
                    className={styles.select}
                    type="text"
                    aria-label={t("settings.instanceNameLabel")}
                    value={settings.instanceName}
                    placeholder="JustDNS"
                    onChange={(e) => update("instanceName", e.target.value)}
                  />
                }
              />
              <Row
                label={t("settings.timezoneLabel")}
                description={t("settings.timezoneDesc")}
                details={t("settings.timezoneDetails")}
                control={
                  <>
                    <input
                      className={styles.select}
                      type="text"
                      list="justdns-timezones"
                      aria-label={t("settings.timezoneLabel")}
                      value={settings.timezone}
                      placeholder="UTC"
                      autoComplete="off"
                      spellCheck={false}
                      onChange={(e) => update("timezone", e.target.value.trim() || "UTC")}
                    />
                    <datalist id="justdns-timezones">
                      {COMMON_TIMEZONES.map((zone) => (
                        <option key={zone} value={zone} />
                      ))}
                    </datalist>
                  </>
                }
              />
              <Row
                label={t("settings.adsLabel")}
                description={t("settings.adsDesc")}
                details={t("settings.adsDetails")}
                control={
                  <Toggle
                    label={t("settings.adsLabel")}
                    checked={settings.blockAdsTrackers}
                    onChange={(v) => update("blockAdsTrackers", v)}
                  />
                }
              />
              <Row
                label={t("settings.malwareLabel")}
                description={t("settings.malwareDesc")}
                details={t("settings.malwareDetails")}
                control={
                  <Toggle
                    label={t("settings.malwareLabel")}
                    checked={settings.blockMalware}
                    onChange={(v) => update("blockMalware", v)}
                  />
                }
              />
              <Row
                label={t("settings.safeSearchLabel")}
                description={t("settings.safeSearchDesc")}
                details={t("settings.safeSearchDetails")}
                control={
                  <Toggle
                    label={t("settings.safeSearchLabel")}
                    checked={settings.safeSearch}
                    onChange={(v) => update("safeSearch", v)}
                  />
                }
              />
              <BlockResponseControl />
            </Category>
          )}

          {active === "blockseite" && (
            <Category
              id="blockseite"
              title={t("settings.cat.blockseite")}
              description={t("settings.blockPage.desc")}
            >
              <BlockPageSection />
            </Category>
          )}

          {active === "benachrichtigungen" && (
            <Category
              id="benachrichtigungen"
              title={t("settings.cat.benachrichtigungen")}
              description={t("settings.notifications.desc")}
            >
              <NotificationsSection />
            </Category>
          )}

          {active === "upstream" && (
            <Category
              id="upstream"
              title={t("settings.cat.upstream")}
              description={t("settings.upstreamDesc")}
            >
              <Row
                label={t("settings.customLabel")}
                description={t("settings.customDesc")}
                details={t("settings.customDetails")}
                control={
                  <Toggle
                    label={t("settings.customLabel")}
                    checked={settings.customUpstreamsEnabled}
                    onChange={(v) => update("customUpstreamsEnabled", v)}
                  />
                }
              />

              {settings.customUpstreamsEnabled ? (
                <CustomUpstreamsEditor />
              ) : (
                <div className={styles.profile}>
                <span className={styles.labelLine}>
                  <span className={styles.label}>{t("settings.profileLabel")}</span>
                  <InfoHint label={t("settings.profileLabel")} text={t("settings.profileDetails")} />
                </span>
                <div
                  className={styles.profileSwitch}
                  role="group"
                  aria-label={t("settings.profileLabel")}
                >
                  {PROFILES.map((profile) => (
                    <button
                      key={profile}
                      type="button"
                      className={styles.profileButton}
                      data-active={settings.speedProfile === profile || undefined}
                      aria-pressed={settings.speedProfile === profile}
                      onClick={() => update("speedProfile", profile)}
                    >
                      {t(`settings.profile.${profile}` as MessageKey)}
                    </button>
                  ))}
                </div>
                <p className={styles.profileDesc}>
                  {t(`settings.profile.${settings.speedProfile}Desc` as MessageKey)}
                </p>

                <span className={styles.resolversTitle}>{t("settings.resolversTitle")}</span>
                <ul className={styles.resolvers}>
                  {PROFILE_RESOLVERS[settings.speedProfile].map((resolver) => (
                    <li key={resolver.ip} className={styles.resolver}>
                      <span className={styles.resolverIp}>{resolver.ip}</span>
                      <span className={styles.resolverHost}>{resolver.host}</span>
                      <span
                        className={styles.resolverTag}
                        data-warn={!settings.encryptUpstream || undefined}
                      >
                        {settings.encryptUpstream ? t("settings.dot") : t("settings.plaintext")}
                      </span>
                    </li>
                  ))}
                </ul>
                </div>
              )}

              {settings.expertMode && (
                <Row
                  label={t("settings.modeLabel")}
                  description={t("settings.modeDesc")}
                  details={t("settings.modeDetails")}
                  control={
                    <select
                      className={styles.select}
                      aria-label={t("settings.modeLabel")}
                      value={settings.upstreamMode}
                      onChange={(e) => update("upstreamMode", e.target.value as UpstreamMode)}
                    >
                      {UPSTREAM_MODES.map((mode) => (
                        <option key={mode} value={mode}>
                          {t(`settings.mode.${mode}` as MessageKey)}
                        </option>
                      ))}
                    </select>
                  }
                />
              )}
            </Category>
          )}

          {active === "verschluesselung" && (
            <Category
              id="verschluesselung"
              title={t("settings.cat.verschluesselung")}
              description={t("settings.enc.desc")}
            >
              {/* Incoming: your own devices reach JustDNS over DoT/DoH. */}
              <h3 className={styles.encGroupTitle}>{t("settings.enc.clientGroup")}</h3>
              <p className={styles.encGroupDesc}>{t("settings.encDns.desc")}</p>
              <EncryptedDnsSection />

              {/* Outgoing: JustDNS encrypts its queries to the upstream resolvers. */}
              <h3 className={styles.encGroupTitle}>{t("settings.enc.upstreamGroup")}</h3>
              <p className={styles.encGroupDesc}>{t("settings.encryptionDesc")}</p>
              <Row
                label={t("settings.encryptLabel")}
                description={t("settings.encryptDesc")}
                details={t("settings.encryptDetails")}
                control={
                  <Toggle
                    label={t("settings.encryptLabel")}
                    checked={settings.encryptUpstream}
                    onChange={(v) => update("encryptUpstream", v)}
                  />
                }
              />
            </Category>
          )}

          {active === "netzwerk" && (
            <Category
              id="netzwerk"
              title={t("settings.cat.netzwerk")}
              description={t("settings.networkDesc")}
            >
              <Row
                label={t("settings.ipv6Label")}
                description={t("settings.ipv6Desc")}
                details={t("settings.ipv6Details")}
                control={
                  <Toggle
                    label={t("settings.ipv6Label")}
                    checked={settings.ipv6}
                    onChange={(v) => update("ipv6", v)}
                  />
                }
              />
              <MetricsSection />
            </Category>
          )}

          {active === "cache" && (
            <Category
              id="cache"
              title={t("settings.cat.cache")}
              description={t("settings.cacheDesc")}
            >
              <Row
                label={t("settings.clearCacheLabel")}
                description={t("settings.clearCacheDesc")}
                details={t("settings.clearCacheDetails")}
                control={<ClearCacheButton />}
              />
            </Category>
          )}

          {active === "sicherung" && (
            <Category
              id="sicherung"
              title={t("settings.cat.sicherung")}
              description={t("settings.backupDesc")}
            >
              <BackupSection />
            </Category>
          )}

          {active === "protokoll" && (
            <Category
              id="protokoll"
              title={t("settings.cat.protokoll")}
              description={t("settings.logDesc")}
            >
              <Row
                label={t("settings.queryLogLabel")}
                description={t("settings.queryLogDesc")}
                details={t("settings.queryLogDetails")}
                control={
                  <Toggle
                    label={t("settings.queryLogLabel")}
                    checked={settings.queryLog}
                    onChange={(v) => update("queryLog", v)}
                  />
                }
              />
              <Row
                label={t("settings.retentionLabel")}
                description={t("settings.retentionDesc")}
                details={t("settings.retentionDetails")}
                disabled={!settings.queryLog}
                control={
                  <label className={styles.number}>
                    <input
                      type="number"
                      min={1}
                      max={365}
                      value={settings.logRetentionDays}
                      disabled={!settings.queryLog}
                      onChange={(e) => update("logRetentionDays", Number(e.target.value))}
                      aria-label={t("settings.retentionAria")}
                    />
                    <span>{t("settings.retentionUnit")}</span>
                  </label>
                }
              />
              <Row
                label={t("settings.activeWindowLabel")}
                description={t("settings.activeWindowDesc")}
                details={t("settings.activeWindowDetails")}
                control={
                  <label className={styles.number}>
                    <input
                      type="number"
                      min={1}
                      max={1440}
                      value={Math.max(1, Math.round(settings.activeWindowSecs / 60))}
                      onChange={(e) =>
                        update("activeWindowSecs", Math.max(1, Number(e.target.value)) * 60)
                      }
                      aria-label={t("settings.activeWindowAria")}
                    />
                    <span>{t("settings.activeWindowUnit")}</span>
                  </label>
                }
              />
            </Category>
          )}
        </div>
      </div>
    </div>
  );
}
