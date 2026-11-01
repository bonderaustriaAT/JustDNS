// Turn the numeric wire values the store keeps (record types, response codes)
// and the lowercase source labels into the names people read. Shared by the
// overview and the log, so a type or code reads the same wherever it appears.
//
// Acronyms (A, NOERROR, …) are the same in every language and stay here as
// literals. The human-readable descriptions and source/block-list names are
// translated: these are plain functions, so they read the active language from
// the i18n module directly rather than through a hook. The hooks that build the
// display rows (useOverview, useLogs) re-run when the language changes, so the
// values below are recomputed in the new language on a switch.

import type { BlockList, QuerySource } from "./api";
import { getLang, translate, type Lang } from "./i18n";
import type { MessageKey } from "./messages";

/** Numeric DNS record types we expect to show; anything else falls back to its
 *  numeric form so an unusual query is still legible. */
const RECORD_TYPES: Record<number, string> = {
  1: "A",
  2: "NS",
  5: "CNAME",
  6: "SOA",
  12: "PTR",
  15: "MX",
  16: "TXT",
  28: "AAAA",
  33: "SRV",
  64: "SVCB",
  65: "HTTPS",
};

/** DNS response codes we name; others fall back to their numeric form. */
const RESPONSE_CODES: Record<number, string> = {
  0: "NOERROR",
  1: "FORMERR",
  2: "SERVFAIL",
  3: "NXDOMAIN",
  4: "NOTIMP",
  5: "REFUSED",
};

/** Record types with a plain-language description in the catalogue; others have
 *  no description line. */
const DESCRIBED_RECORD_TYPES = new Set([1, 2, 5, 6, 12, 15, 16, 28, 33, 64, 65]);

export function recordTypeLabel(code: number): string {
  return RECORD_TYPES[code] ?? `TYPE${code}`;
}

export function recordTypeDescription(code: number, lang: Lang = getLang()): string | undefined {
  if (!DESCRIBED_RECORD_TYPES.has(code)) {
    return undefined;
  }
  return translate(`rtypeDesc.${code}` as MessageKey, undefined, lang);
}

export function responseCodeLabel(code: number): string {
  return RESPONSE_CODES[code] ?? `RCODE${code}`;
}

export function responseCodeDescription(code: number, lang: Lang = getLang()): string | undefined {
  if (!(code in RESPONSE_CODES)) {
    return undefined;
  }
  return translate(`rcodeDesc.${code}` as MessageKey, undefined, lang);
}

export function sourceLabel(source: QuerySource, lang: Lang = getLang()): string {
  return translate(`source.${source}` as MessageKey, undefined, lang);
}

export function blockListLabel(list: BlockList, lang: Lang = getLang()): string {
  return translate(`blocklist.${list}` as MessageKey, undefined, lang);
}

/** Strip the trailing dot from a fully qualified name for display. */
export function displayName(name: string): string {
  return name.replace(/\.$/, "");
}
