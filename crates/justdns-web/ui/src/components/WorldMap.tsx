// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The world map itself is a bundled asset, src/assets/world-map.svg, by
// Al MacDonald (original artwork) and Fritz Lekschas (editor), under CC BY-SA 3.0.
// It keeps its own <title>/<desc> attribution in the SVG; the credit is also shown
// under the map in the UI and documented in the feature guides.

import { useEffect, useRef } from "react";

import worldMapRaw from "../assets/world-map.svg?raw";
import { countryName } from "../lib/geo";
import type { useI18n } from "../lib/i18n";
import styles from "./WorldMap.module.css";

/** One country that the answers resolved into, with its total resolved contacts
 *  over the window. `code` is a lowercase ISO 3166-1 alpha-2 code, matching the
 *  path/group ids in the bundled SVG. */
export interface MapCountry {
  code: string;
  contacts: number;
}

/** Strip the XML prolog and DOCTYPE so the markup can be injected into the HTML
 *  document; everything from the opening <svg> onward (including its attribution
 *  <title>/<desc>) is kept. Computed once at module load. */
const SVG_MARKUP = worldMapRaw.slice(Math.max(0, worldMapRaw.indexOf("<svg")));

/** A selector-safe form of a two-letter country code. The codes we pass are
 *  already validated as [a-z]{2}, but go through CSS.escape where available. */
function idSelector(code: string): string {
  const escaped =
    typeof CSS !== "undefined" && typeof CSS.escape === "function" ? CSS.escape(code) : code;
  return `#${escaped}`;
}

/** The SVG <path> elements that make up one country: the element itself when it
 *  is a single path, or every descendant path when it is a multi-part group. */
function countryPaths(root: Element): SVGPathElement[] {
  if (root.tagName.toLowerCase() === "path") return [root as SVGPathElement];
  return Array.from(root.querySelectorAll<SVGPathElement>("path"));
}

/** An offline choropleth of the countries the network's answers resolved into,
 *  coloured by contact volume. Clicking a contacted country selects it (click it
 *  again, or click elsewhere, to clear); selection drives the filtered network
 *  list beside the map. No map library and no network calls — the SVG is bundled
 *  and the colouring is done in the DOM. */
export function WorldMap({
  countries,
  max,
  selected,
  onSelect,
  locale,
  t,
  nf,
}: {
  countries: MapCountry[];
  max: number;
  selected: string | null;
  onSelect: (code: string | null) => void;
  locale: string;
  t: ReturnType<typeof useI18n>["t"];
  nf: Intl.NumberFormat;
}) {
  const hostRef = useRef<HTMLDivElement>(null);

  // Colour and label the contacted countries. Re-runs when the data (or its
  // scale) changes — e.g. when the window switches — resetting the previous pass
  // first so no stale colour or tooltip lingers.
  useEffect(() => {
    const svg = hostRef.current?.querySelector("svg");
    if (!svg) return;

    svg.querySelectorAll<SVGElement>("[data-contact]").forEach((el) => {
      el.style.fill = "";
      el.removeAttribute("data-contact");
    });
    svg.querySelectorAll("title[data-map-title]").forEach((node) => node.remove());

    for (const { code, contacts } of countries) {
      const root = svg.querySelector<SVGElement>(idSelector(code));
      if (!root) continue;
      // A perceptual square-root ramp so a handful of contacts is still visible
      // next to a very busy country; from a faint land tint toward the signal.
      const ratio = max > 0 ? Math.sqrt(contacts / max) : 0;
      const pct = (16 + 68 * ratio).toFixed(1);
      const fill = `color-mix(in srgb, var(--map-hot) ${pct}%, var(--map-land))`;
      for (const path of countryPaths(root)) {
        path.style.fill = fill;
        path.setAttribute("data-contact", "1");
      }
      const title = document.createElementNS("http://www.w3.org/2000/svg", "title");
      title.setAttribute("data-map-title", "1");
      title.textContent = `${countryName(code, locale) || code.toUpperCase()}: ${nf.format(contacts)}`;
      root.insertBefore(title, root.firstChild);
    }
  }, [countries, max, locale, nf]);

  // Mark the selected country so CSS can ring it. Separate from colouring so
  // re-selecting does not recolour the whole map.
  useEffect(() => {
    const svg = hostRef.current?.querySelector("svg");
    if (!svg) return;
    svg
      .querySelectorAll<SVGElement>("[data-selected]")
      .forEach((el) => el.removeAttribute("data-selected"));
    if (selected) {
      svg.querySelector<SVGElement>(idSelector(selected))?.setAttribute("data-selected", "1");
    }
  }, [selected, countries]);

  const hasData = (code: string) => countries.some((c) => c.code === code);

  function handleClick(event: React.MouseEvent) {
    const target = (event.target as Element).closest<Element>("[id]");
    const code = target?.id;
    if (code && /^[a-z]{2}$/.test(code) && hasData(code)) {
      onSelect(code === selected ? null : code);
    } else {
      // A click on the ocean or a country with no contacts clears the filter.
      onSelect(null);
    }
  }

  return (
    <div className={styles.wrap}>
      {/* eslint-disable-next-line react/no-danger -- bundled, script-free asset */}
      <div
        ref={hostRef}
        className={styles.map}
        role="img"
        aria-label={t("privacy.mapAria")}
        onClick={handleClick}
        dangerouslySetInnerHTML={{ __html: SVG_MARKUP }}
      />
      <div className={styles.legend}>
        <span className={styles.legendText}>{t("privacy.mapLegendLow")}</span>
        <span className={styles.legendBar} aria-hidden="true" />
        <span className={styles.legendText}>{t("privacy.mapLegendHigh")}</span>
        <span className={styles.legendLabel}>{t("privacy.mapLegendLabel")}</span>
      </div>
    </div>
  );
}
