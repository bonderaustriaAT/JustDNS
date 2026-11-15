// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { api } from "./api";

/** Trigger a browser download of the current configuration as an encrypted
 *  restore file, sealed with the password the admin typed. Shared by the
 *  single-node update card and the cluster-update flow so an admin always walks
 *  away from an update with a restore file in hand, whichever path they took. */
export async function downloadRestore(password: string): Promise<void> {
  const blob = await api.downloadBackup(password);
  const url = URL.createObjectURL(blob);
  const stamp = new Date().toISOString().slice(0, 10);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = `justdns-restore-${stamp}.jdbak`;
  anchor.style.display = "none";
  document.body.appendChild(anchor);
  anchor.click();
  // Clean up on a later task, never in the same tick as click(): the browser
  // reads the object URL for the download asynchronously, so revoking it (or
  // removing the anchor) synchronously can cancel the download before it starts
  // and the file silently never arrives. This was the only download the rolling
  // cluster update triggered, so there it looked like "no backup at all".
  window.setTimeout(() => {
    anchor.remove();
    URL.revokeObjectURL(url);
  }, 40_000);
}
