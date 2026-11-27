// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

/** Focus helpers shared by the two dialog components, so keyboard trapping
 *  cannot drift between them: the centred Modal and the destructive
 *  ConfirmDialog trap Tab the same way and restore focus the same way. */

/** Everything inside a dialog that can hold focus, in tab order. */
export const FOCUSABLE =
  'input, select, textarea, button, [href], [tabindex]:not([tabindex="-1"])';

/** Move focus into the first focusable element of `node`. */
export function focusFirst(node: HTMLElement | null) {
  node?.querySelector<HTMLElement>(FOCUSABLE)?.focus();
}

/** Keep Tab inside `node` while a dialog is open: a Tab on the last element
 *  wraps to the first, a Shift-Tab on the first wraps back to the last. Disabled
 *  controls are skipped, so an in-flight action never becomes a focus stop.
 *  Returns whether the event was handled (the caller then prevents default). */
export function trapTab(node: HTMLElement, event: KeyboardEvent): boolean {
  if (event.key !== "Tab") return false;
  const items = Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) => !el.hasAttribute("disabled"),
  );
  if (items.length === 0) return false;
  const first = items[0];
  const last = items[items.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    last.focus();
    return true;
  }
  if (!event.shiftKey && document.activeElement === last) {
    first.focus();
    return true;
  }
  return false;
}
