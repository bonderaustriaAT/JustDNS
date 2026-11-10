/** Helpers for carrying a (possibly encrypted) backup file through the JSON API.
 *  A backup is downloaded as raw bytes; to send it back for a restore it is
 *  base64-encoded, and the UI needs to know whether it is password-encrypted so
 *  it can ask for the password. */

/** Base64-encode raw bytes. Backups are small, so a simple loop is plenty. */
export function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i += 1) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

/** The magic marker a password-encrypted backup begins with ("JDBAK1"). */
const BACKUP_MAGIC = [0x4a, 0x44, 0x42, 0x41, 0x4b, 0x31];

/** True when the bytes start with the encrypted-backup marker. A legacy
 *  plaintext JSON backup does not, so it restores without a password. */
export function looksEncrypted(bytes: Uint8Array): boolean {
  if (bytes.length < BACKUP_MAGIC.length) return false;
  return BACKUP_MAGIC.every((b, i) => bytes[i] === b);
}
