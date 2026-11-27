import type { ReasonCategory, TraceOutcome, TraceStage } from "./api";
import type { MessageKey } from "./messages";

/** The message key for a reason category's label (the log row's reason tag). */
export const REASON_KEY: Record<ReasonCategory, MessageKey> = {
  blocklist: "reason.blocklist",
  phishing: "reason.phishing",
  threat_ip: "reason.threat_ip",
  warning: "reason.warning",
};

/** The message key for a trace stage's name. */
export const STAGE_KEY: Record<TraceStage, MessageKey> = {
  name_block: "trace.stage.name_block",
  phishing: "trace.stage.phishing",
  source: "trace.stage.source",
  cname_uncloak: "trace.stage.cname_uncloak",
  response_ip: "trace.stage.response_ip",
  dnssec: "trace.stage.dnssec",
  answer: "trace.stage.answer",
};

/** The message key for a trace step's outcome label. */
export const OUTCOME_KEY: Record<TraceOutcome, MessageKey> = {
  pass: "trace.outcome.pass",
  hit: "trace.outcome.hit",
  warn: "trace.outcome.warn",
  info: "trace.outcome.info",
};

/** Where a decisive stage links to so the rule behind it can be managed, and the
 *  label for that link. A name or CNAME-cloak block goes to the user's own lists
 *  (focused on the allow editor), a response-IP block to the threat-IP presets,
 *  a phishing hit to the phishing page. Stages with no governing page are
 *  absent. */
export const STAGE_LINK: Partial<Record<TraceStage, { to: string; label: MessageKey }>> = {
  name_block: { to: "/listen/url?focus=allow", label: "trace.linkLists" },
  cname_uncloak: { to: "/listen/url?focus=allow", label: "trace.linkLists" },
  phishing: { to: "/phishing", label: "trace.linkPhishing" },
  response_ip: { to: "/listen/fertige?focus=threatip", label: "trace.linkThreatIp" },
};
