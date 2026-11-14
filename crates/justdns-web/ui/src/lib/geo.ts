// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { GeoInfo } from "./api";

/** Turn an ISO 3166-1 alpha-2 country code into its flag emoji by mapping each
 *  letter to its regional-indicator symbol. Returns an empty string for anything
 *  that is not two ASCII letters, so a malformed code degrades to just text. */
export function flagEmoji(code: string | undefined): string {
  if (!code || code.length !== 2 || !/^[A-Za-z]{2}$/.test(code)) return "";
  const base = 0x1f1e6;
  const upper = code.toUpperCase();
  return (
    String.fromCodePoint(base + upper.charCodeAt(0) - 65) +
    String.fromCodePoint(base + upper.charCodeAt(1) - 65)
  );
}

/** The localised country name for an ISO code, via the browser's own
 *  `Intl.DisplayNames` — no bundled name table. Falls back to the upper-cased
 *  code when the runtime cannot name it. */
export function countryName(code: string | undefined, locale: string): string {
  if (!code) return "";
  try {
    const names = new Intl.DisplayNames([locale], { type: "region" });
    return names.of(code.toUpperCase()) ?? code.toUpperCase();
  } catch {
    return code.toUpperCase();
  }
}

/** A compact one-line label for a resolved network: flag, the operator name when
 *  known (else the country name), and the ASN number. For example
 *  "🇺🇸 Cloudflare · AS13335". Used in the log's geo cell. */
export function describeGeo(info: GeoInfo, locale: string): string {
  const flag = flagEmoji(info.country);
  const primary = info.org ?? countryName(info.country, locale);
  const asn = info.asn != null ? `AS${info.asn}` : "";
  const parts = [primary, asn].filter(Boolean);
  const text = parts.join(" · ");
  return flag ? `${flag} ${text}`.trim() : text;
}
