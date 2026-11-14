// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useState } from "react";

import { ConfirmDialog } from "../components/ConfirmDialog";
import { Modal } from "../components/Modal";
import { WriteGuard } from "../components/WriteGuard";
import { ApiError, api, type CertListItem } from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import styles from "./Certificates.module.css";

/** Certificates within this many days of expiry get a "soon" badge, so an
 *  operator renews before a client starts rejecting the handshake. */
const SOON_DAYS = 30;

/** The feature ids the API reports in `usedBy`, mapped to their label keys. */
const USED_BY_LABEL: Record<string, MessageKey> = {
  dot: "certs.usedDot",
  doh: "certs.usedDoh",
  blockPage: "certs.usedBlockPage",
};

/** Format an expiry instant in the active locale, date only. */
function formatExpiry(ms: number, locale: string): string {
  return new Date(ms).toLocaleDateString(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

/** The add-certificate form: a name plus either "generate a self-signed one" or
 *  "upload a chain and key". Returns the refreshed catalogue to the parent. */
function AddCertificate({ onSaved }: { onSaved: (list: CertListItem[]) => void }) {
  const { t } = useI18n();
  const [mode, setMode] = useState<"generate" | "upload">("generate");
  const [name, setName] = useState("");
  const [hostname, setHostname] = useState("");
  const [cert, setCert] = useState("");
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const canSubmit =
    name.trim() !== "" && (mode === "generate" || (cert.trim() !== "" && key.trim() !== ""));

  async function submit() {
    setBusy(true);
    setError(null);
    try {
      const list =
        mode === "generate"
          ? await api.generateCert(name.trim(), hostname.trim() || undefined)
          : await api.uploadCert(name.trim(), cert, key);
      onSaved(list);
      setName("");
      setHostname("");
      setCert("");
      setKey("");
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("certs.saveError"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className={styles.addCard} aria-label={t("certs.addTitle")}>
      <h2 className={styles.addTitle}>{t("certs.addTitle")}</h2>

      <div className={styles.modeSwitch} role="group" aria-label={t("certs.addTitle")}>
        {(["generate", "upload"] as const).map((m) => (
          <button
            key={m}
            type="button"
            className={styles.modeButton}
            data-active={mode === m || undefined}
            aria-pressed={mode === m}
            onClick={() => setMode(m)}
          >
            {t(m === "generate" ? "certs.modeGenerate" : "certs.modeUpload")}
          </button>
        ))}
      </div>

      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t("certs.nameLabel")}</span>
        <input
          className={styles.input}
          type="text"
          value={name}
          placeholder={t("certs.namePlaceholder")}
          onChange={(e) => setName(e.target.value)}
        />
      </label>

      {mode === "generate" ? (
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("certs.hostnameLabel")}</span>
          <input
            className={styles.input}
            type="text"
            value={hostname}
            placeholder={t("certs.hostnamePlaceholder")}
            spellCheck={false}
            onChange={(e) => setHostname(e.target.value)}
          />
          <span className={styles.hint}>{t("certs.hostnameHint")}</span>
        </label>
      ) : (
        <>
          <label className={styles.field}>
            <span className={styles.fieldLabel}>{t("certs.certField")}</span>
            <textarea
              className={styles.textarea}
              value={cert}
              rows={4}
              spellCheck={false}
              placeholder="-----BEGIN CERTIFICATE-----"
              onChange={(e) => setCert(e.target.value)}
            />
          </label>
          <label className={styles.field}>
            <span className={styles.fieldLabel}>{t("certs.keyField")}</span>
            <textarea
              className={styles.textarea}
              value={key}
              rows={4}
              spellCheck={false}
              placeholder="-----BEGIN PRIVATE KEY-----"
              onChange={(e) => setKey(e.target.value)}
            />
          </label>
        </>
      )}

      {error ? (
        <p className={styles.formError} role="alert">
          {error}
        </p>
      ) : null}

      <div className={styles.addActions}>
        <button
          type="button"
          className={styles.primaryButton}
          onClick={submit}
          disabled={!canSubmit || busy}
        >
          {busy
            ? t("certs.busy")
            : t(mode === "generate" ? "certs.generateButton" : "certs.uploadButton")}
        </button>
      </div>
    </section>
  );
}

/** The renew modal: upload a replacement chain and key for one certificate. The
 *  server validates it is currently valid before swapping it in place. */
function RenewModal({
  target,
  onClose,
  onSaved,
}: {
  target: CertListItem;
  onClose: () => void;
  onSaved: (list: CertListItem[]) => void;
}) {
  const { t } = useI18n();
  const [cert, setCert] = useState("");
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit() {
    setBusy(true);
    setError(null);
    try {
      const list = await api.renewCert(target.id, cert, key);
      onSaved(list);
      onClose();
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("certs.saveError"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal open onClose={onClose} title={t("certs.renewTitle")}>
      <p className={styles.modalIntro}>{t("certs.renewIntro", { name: target.name })}</p>
      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t("certs.certField")}</span>
        <textarea
          className={styles.textarea}
          value={cert}
          rows={4}
          spellCheck={false}
          placeholder="-----BEGIN CERTIFICATE-----"
          onChange={(e) => setCert(e.target.value)}
        />
      </label>
      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t("certs.keyField")}</span>
        <textarea
          className={styles.textarea}
          value={key}
          rows={4}
          spellCheck={false}
          placeholder="-----BEGIN PRIVATE KEY-----"
          onChange={(e) => setKey(e.target.value)}
        />
      </label>
      {error ? (
        <p className={styles.formError} role="alert">
          {error}
        </p>
      ) : null}
      <div className={styles.modalActions}>
        <button type="button" className={styles.ghostButton} onClick={onClose} disabled={busy}>
          {t("certs.cancel")}
        </button>
        <button
          type="button"
          className={styles.primaryButton}
          onClick={submit}
          disabled={busy || cert.trim() === "" || key.trim() === ""}
        >
          {busy ? t("certs.busy") : t("certs.renewButton")}
        </button>
      </div>
    </Modal>
  );
}

export function Certificates() {
  const { t, lang } = useI18n();
  const [certs, setCerts] = useState<CertListItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [renewTarget, setRenewTarget] = useState<CertListItem | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<CertListItem | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    api
      .certs()
      .then((list) => !cancelled && setCerts(list))
      .catch(() => {
        if (!cancelled) {
          setCerts([]);
          setError(t("certs.loadError"));
        }
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function downloadCa(cert: CertListItem) {
    try {
      const blob = await api.downloadCertCa(cert.id);
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `${cert.name || cert.id}-ca.pem`;
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      URL.revokeObjectURL(url);
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("certs.saveError"));
    }
  }

  async function confirmDelete() {
    if (!deleteTarget) return;
    setDeleteBusy(true);
    setError(null);
    try {
      const list = await api.deleteCert(deleteTarget.id);
      setCerts(list);
      setDeleteTarget(null);
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("certs.saveError"));
    } finally {
      setDeleteBusy(false);
    }
  }

  // An empty list after a successful load would be unusual (the store keeps a
  // default), so treat it as the store being unavailable.
  const storeUnavailable = certs !== null && certs.length === 0 && !error;
  const soonCutoff = Date.now() + SOON_DAYS * 24 * 60 * 60 * 1000;

  return (
    <WriteGuard>
    <div className={styles.page}>
      <div className={styles.header}>
        <h1 className={styles.title}>{t("certs.title")}</h1>
      </div>
      <p className={styles.lead}>{t("certs.intro")}</p>

      {error ? (
        <p className={styles.banner} role="alert">
          {error}
        </p>
      ) : null}

      {certs === null ? (
        <p className={styles.lead}>{t("common.loading")}</p>
      ) : storeUnavailable ? (
        <p className={styles.banner}>{t("certs.unavailable")}</p>
      ) : (
        <>
          {certs.length === 0 ? (
            <p className={styles.empty}>{t("certs.empty")}</p>
          ) : (
            <div className={styles.tableWrap}>
              <table className={styles.table}>
                <thead>
                  <tr>
                    <th>{t("certs.colName")}</th>
                    <th>{t("certs.colType")}</th>
                    <th>{t("certs.colSubject")}</th>
                    <th>{t("certs.colExpiry")}</th>
                    <th>{t("certs.colFingerprint")}</th>
                    <th>{t("certs.colUsedBy")}</th>
                    <th className={styles.actionsHead}>{t("certs.colActions")}</th>
                  </tr>
                </thead>
                <tbody>
                  {certs.map((cert) => {
                    const soon = !cert.expired && cert.notAfterMs <= soonCutoff;
                    return (
                      <tr key={cert.id}>
                        <td className={styles.nameCell}>{cert.name}</td>
                        <td>
                          {t(cert.kind === "auto" ? "certs.kindAuto" : "certs.kindUploaded")}
                        </td>
                        <td className={styles.subjectCell} title={cert.subject}>
                          {cert.subject || "—"}
                        </td>
                        <td className={styles.expiryCell}>
                          <span>{formatExpiry(cert.notAfterMs, lang)}</span>
                          {cert.expired ? (
                            <span className={styles.badgeExpired}>{t("certs.badgeExpired")}</span>
                          ) : soon ? (
                            <span className={styles.badgeSoon}>{t("certs.badgeSoon")}</span>
                          ) : null}
                        </td>
                        <td className={styles.fingerprintCell} title={cert.fingerprint}>
                          {cert.fingerprint}
                        </td>
                        <td className={styles.usedCell}>
                          {cert.usedBy.length === 0 ? (
                            <span className={styles.usedNone}>{t("certs.usedByNone")}</span>
                          ) : (
                            <span className={styles.usedTags}>
                              {cert.usedBy.map((id) => (
                                <span key={id} className={styles.usedTag}>
                                  {USED_BY_LABEL[id] ? t(USED_BY_LABEL[id]) : id}
                                </span>
                              ))}
                            </span>
                          )}
                        </td>
                        <td className={styles.actionsCell}>
                          {cert.hasCa ? (
                            <button
                              type="button"
                              className={styles.rowButton}
                              onClick={() => downloadCa(cert)}
                            >
                              {t("certs.downloadCa")}
                            </button>
                          ) : null}
                          <button
                            type="button"
                            className={styles.rowButton}
                            onClick={() => setRenewTarget(cert)}
                          >
                            {t("certs.renew")}
                          </button>
                          <button
                            type="button"
                            className={styles.rowButtonDanger}
                            onClick={() => setDeleteTarget(cert)}
                            disabled={cert.usedBy.length > 0}
                          >
                            {t("certs.delete")}
                          </button>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}

          <AddCertificate onSaved={setCerts} />
        </>
      )}

      {renewTarget ? (
        <RenewModal
          target={renewTarget}
          onClose={() => setRenewTarget(null)}
          onSaved={setCerts}
        />
      ) : null}

      {deleteTarget ? (
        <ConfirmDialog
          title={t("certs.deleteConfirmTitle")}
          body={t("certs.deleteConfirmBody", { name: deleteTarget.name })}
          confirmLabel={t("certs.deleteConfirmAction")}
          cancelLabel={t("certs.cancel")}
          busy={deleteBusy}
          onConfirm={confirmDelete}
          onCancel={() => setDeleteTarget(null)}
        />
      ) : null}
    </div>
    </WriteGuard>
  );
}
