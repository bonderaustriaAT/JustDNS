import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type ReactNode,
} from "react";

import { ConfirmDialog } from "../components/ConfirmDialog";
import { InfoHint } from "../components/InfoHint";
import { Toggle } from "../components/Toggle";
import { ApiError, api, type CertSource, type TlsSettings, type TlsStatus } from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import {
  useSettings,
  type BlockResponse,
  type CustomTransport,
  type CustomUpstream,
  type SpeedProfile,
  type UpstreamMode,
} from "../state/settings";
import styles from "./Settings.module.css";

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

/** Export the whole configuration to a file, and restore one. Export downloads
 *  a JSON backup; restore reads a chosen file, confirms, applies it on the
 *  server and reloads so every view reflects the restored data. */
function BackupSection() {
  const { t } = useI18n();
  const fileRef = useRef<HTMLInputElement>(null);
  const [exportState, setExportState] = useState<"idle" | "busy" | "error">("idle");
  const [importState, setImportState] = useState<"idle" | "busy" | "done" | "error">("idle");
  const [importError, setImportError] = useState<string | null>(null);
  // The parsed backup awaiting the confirm step, or null when none is pending.
  const [pending, setPending] = useState<unknown>(null);

  async function download() {
    setExportState("busy");
    try {
      const blob = await api.downloadBackup();
      const url = URL.createObjectURL(blob);
      const stamp = new Date().toISOString().slice(0, 10);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `justdns-backup-${stamp}.json`;
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      URL.revokeObjectURL(url);
      setExportState("idle");
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
    try {
      const parsed: unknown = JSON.parse(await file.text());
      setPending(parsed);
    } catch {
      setImportState("error");
      setImportError(t("settings.backupImportInvalid"));
    }
  }

  async function confirmRestore() {
    setImportState("busy");
    try {
      await api.restoreBackup(pending);
      setPending(null);
      setImportState("done");
      // Reload so settings, lists and rewrites views all show the restored data.
      window.setTimeout(() => window.location.reload(), 1200);
    } catch (error) {
      setPending(null);
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
          <div className={styles.action}>
            <button
              type="button"
              className={styles.actionButton}
              onClick={download}
              disabled={exportState === "busy"}
            >
              {exportState === "busy"
                ? t("settings.backupExportBusy")
                : t("settings.backupExportButton")}
            </button>
            {exportState === "error" ? (
              <span className={styles.actionError}>{t("settings.backupExportError")}</span>
            ) : null}
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
              accept="application/json,.json"
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
          confirmLabel={t("settings.backupRestoreConfirm")}
          cancelLabel={t("settings.backupRestoreCancel")}
          busy={importState === "busy"}
          onConfirm={confirmRestore}
          onCancel={() => setPending(null)}
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
  description: string;
  details: string;
  control: ReactNode;
  disabled?: boolean;
}) {
  return (
    <div className={styles.row} data-disabled={disabled || undefined}>
      <div className={styles.text}>
        <span className={styles.labelLine}>
          <span className={styles.label}>{label}</span>
          <InfoHint label={label} text={details} />
        </span>
        <span className={styles.description}>{description}</span>
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

/** The certificate sources, in the order the switch lists them. */
const CERT_SOURCES: CertSource[] = ["selfSigned", "uploaded"];

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
  const [cert, setCert] = useState("");
  const [key, setKey] = useState("");
  const [uploadState, setUploadState] = useState<"idle" | "busy" | "done" | "error">("idle");
  const [regenState, setRegenState] = useState<"idle" | "busy" | "done">("idle");
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
      certSource: status.certSource,
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

  async function downloadCa() {
    try {
      const blob = await api.downloadCa();
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = "justdns-ca.pem";
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      URL.revokeObjectURL(url);
    } catch {
      setError(t("settings.encDns.saveError"));
    }
  }

  async function uploadCert() {
    setUploadState("busy");
    setError(null);
    try {
      const updated = await api.uploadCert(cert, key);
      setStatus(updated);
      setCert("");
      setKey("");
      setUploadState("done");
      window.setTimeout(() => setUploadState("idle"), 2500);
    } catch (e) {
      setUploadState("error");
      setError(e instanceof ApiError ? e.message : t("settings.encDns.saveError"));
    }
  }

  async function regenerate() {
    setRegenState("busy");
    setError(null);
    try {
      const updated = await api.regenerateCert();
      setStatus(updated);
      setRegenState("done");
      window.setTimeout(() => setRegenState("idle"), 2500);
    } catch (e) {
      setRegenState("idle");
      setError(e instanceof ApiError ? e.message : t("settings.encDns.saveError"));
    }
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

      {/* Certificate and endpoint addresses only make sense once an endpoint is
          actually running — hidden until then to keep the page focused. */}
      {featureActive ? (
        <>
      <div className={styles.profile}>
        <span className={styles.labelLine}>
          <span className={styles.label}>{t("settings.encDns.certSourceLabel")}</span>
          <InfoHint
            label={t("settings.encDns.certSourceLabel")}
            text={t("settings.encDns.certSourceDetails")}
          />
        </span>
        <div
          className={styles.profileSwitch}
          role="group"
          aria-label={t("settings.encDns.certSourceLabel")}
        >
          {CERT_SOURCES.map((source) => (
            <button
              key={source}
              type="button"
              className={styles.profileButton}
              data-active={status.certSource === source || undefined}
              aria-pressed={status.certSource === source}
              onClick={() => patch({ certSource: source })}
            >
              {t(
                source === "selfSigned"
                  ? "settings.encDns.certSelfSigned"
                  : "settings.encDns.certUploaded",
              )}
            </button>
          ))}
        </div>
        <p className={styles.profileDesc}>
          {t(
            status.certSource === "selfSigned"
              ? "settings.encDns.certSelfSignedDesc"
              : "settings.encDns.certUploadedDesc",
          )}
        </p>
      </div>

      {status.fingerprint ? (
        <div className={styles.encFingerprint}>
          <span className={styles.label}>{t("settings.encDns.fingerprintLabel")}</span>
          <code className={styles.encCode}>{status.fingerprint}</code>
        </div>
      ) : null}

      {status.certSource === "selfSigned" ? (
        <div className={styles.encCertActions}>
          <div className={styles.action}>
            <button
              type="button"
              className={styles.actionButton}
              onClick={downloadCa}
              disabled={!status.caAvailable}
            >
              {t("settings.encDns.caDownload")}
            </button>
            <button
              type="button"
              className={styles.actionButton}
              onClick={regenerate}
              disabled={regenState === "busy"}
            >
              {regenState === "busy"
                ? t("settings.encDns.regenerateBusy")
                : regenState === "done"
                  ? t("settings.encDns.regenerateDone")
                  : t("settings.encDns.regenerateButton")}
            </button>
          </div>
          <p className={styles.encHint}>{t("settings.encDns.caHint")}</p>
        </div>
      ) : (
        <div className={styles.encUpload}>
          <label className={styles.encField}>
            <span className={styles.label}>{t("settings.encDns.uploadCertField")}</span>
            <textarea
              className={styles.encTextarea}
              value={cert}
              onChange={(e) => setCert(e.target.value)}
              rows={4}
              spellCheck={false}
              placeholder="-----BEGIN CERTIFICATE-----"
            />
          </label>
          <label className={styles.encField}>
            <span className={styles.label}>{t("settings.encDns.uploadKeyField")}</span>
            <textarea
              className={styles.encTextarea}
              value={key}
              onChange={(e) => setKey(e.target.value)}
              rows={4}
              spellCheck={false}
              placeholder="-----BEGIN PRIVATE KEY-----"
            />
          </label>
          <div className={styles.action}>
            <button
              type="button"
              className={styles.actionButton}
              onClick={uploadCert}
              disabled={uploadState === "busy" || !cert.trim() || !key.trim()}
            >
              {uploadState === "busy"
                ? t("settings.encDns.uploadBusy")
                : uploadState === "done"
                  ? t("settings.encDns.uploadDone")
                  : t("settings.encDns.uploadButton")}
            </button>
          </div>
        </div>
      )}

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

export function Settings() {
  const { t } = useI18n();
  const { settings, update } = useSettings();
  const [selected, setSelected] = useState("schutz");

  const categories = useMemo<CategoryMeta[]>(() => {
    // "Upstream" now hosts the speed profile — the headline user-facing choice —
    // so it stays visible even outside expert mode; the rest stay technical.
    const base: CategoryMeta[] = [
      { id: "schutz", titleKey: "settings.cat.schutz" },
      { id: "upstream", titleKey: "settings.cat.upstream" },
      // Backup & restore stays visible even outside expert mode: making a
      // safety copy is a headline user-facing action, not a technical knob.
      { id: "sicherung", titleKey: "settings.cat.sicherung" },
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
