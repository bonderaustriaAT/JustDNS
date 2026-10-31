// Turn the numeric wire values the store keeps (record types, response codes)
// and the lowercase source labels into the names people read. Shared by the
// overview and the log, so a type or code reads the same wherever it appears.

import type { QuerySource } from "./api";

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
};

export function recordTypeLabel(code: number): string {
  return RECORD_TYPES[code] ?? `TYPE${code}`;
}

export function responseCodeLabel(code: number): string {
  return RESPONSE_CODES[code] ?? `RCODE${code}`;
}

export function sourceLabel(source: QuerySource): string {
  return SOURCE_LABELS[source] ?? source;
}

/** Strip the trailing dot from a fully qualified name for display. */
export function displayName(name: string): string {
  return name.replace(/\.$/, "");
}
