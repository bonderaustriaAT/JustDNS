// Turn the numeric wire values the store keeps (record types, response codes)
// and the lowercase source labels into the names people read. Shared by the
// overview and the log, so a type or code reads the same wherever it appears.

import type { BlockList, QuerySource } from "./api";

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

const SOURCE_LABELS: Record<QuerySource, string> = {
  cache: "Cache",
  upstream: "Upstream",
  local: "Lokal",
  rewrite: "Rewrite",
  blocked: "Geblockt",
};

/** Plain-language meaning of a record type, for the detail view. */
const RECORD_DESCRIPTIONS: Record<number, string> = {
  1: "IPv4-Adresse",
  2: "Nameserver",
  5: "Alias auf einen anderen Namen",
  6: "Zonen-Stammeintrag",
  12: "Reverse-Lookup (Adresse → Name)",
  15: "Mailserver",
  16: "Texteintrag",
  28: "IPv6-Adresse",
  33: "Dienst-Standort",
  64: "Dienst-Parameter",
  65: "HTTPS-Diensteintrag",
};

/** Plain-language meaning of a response code, so the detail view can explain
 *  what happened instead of just the acronym. */
const RESPONSE_DESCRIPTIONS: Record<number, string> = {
  0: "Erfolgreich beantwortet",
  1: "Fehlerhafte Anfrage",
  2: "Auflösung fehlgeschlagen",
  3: "Name existiert nicht",
  4: "Abfragetyp nicht unterstützt",
  5: "Abfrage abgelehnt",
};

/** German names for the block lists. */
const BLOCK_LIST_LABELS: Record<BlockList, string> = {
  ads: "Werbung & Tracker",
  malware: "Schadsoftware",
  custom: "Eigene Blockliste",
};

export function recordTypeLabel(code: number): string {
  return RECORD_TYPES[code] ?? `TYPE${code}`;
}

export function recordTypeDescription(code: number): string | undefined {
  return RECORD_DESCRIPTIONS[code];
}

export function responseCodeLabel(code: number): string {
  return RESPONSE_CODES[code] ?? `RCODE${code}`;
}

export function responseCodeDescription(code: number): string | undefined {
  return RESPONSE_DESCRIPTIONS[code];
}

export function sourceLabel(source: QuerySource): string {
  return SOURCE_LABELS[source] ?? source;
}

export function blockListLabel(list: BlockList): string {
  return BLOCK_LIST_LABELS[list] ?? list;
}

/** Strip the trailing dot from a fully qualified name for display. */
export function displayName(name: string): string {
  return name.replace(/\.$/, "");
}
