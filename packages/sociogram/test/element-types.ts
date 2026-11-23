// Type-level checks for the Web Component, compiled by `npm run typecheck`
// (it is not a vitest file: nothing here runs). If a line stops compiling, or a
// `@ts-expect-error` line starts compiling, the public types changed.

import type { SociogramElement } from "../src/element.js";

declare const el: SociogramElement;

// Events are typed, so `detail` needs no cast.
el.addEventListener("nodeclick", (e) => {
  const index: number = e.detail;
  void index;
});
el.addEventListener("nodehover", (e) => {
  const index: number | null = e.detail;
  void index;
});
el.addEventListener("error", (e) => {
  // `error` is a CustomEvent here, not the DOM's ErrorEvent.
  const message: string = e.detail.error.message;
  const phase: "create" | "data" | "render" | "callback" = e.detail.phase;
  void message;
  void phase;
});
el.addEventListener("ready", (e) => {
  e.detail.fitView();
});

// The wrong type for `detail` is rejected.
el.addEventListener("nodeclick", (e) => {
  // @ts-expect-error detail is a number, not a string
  const wrong: string = e.detail;
  void wrong;
});

// Other events keep working with their usual types.
el.addEventListener("click", (e: MouseEvent) => void e.clientX);
el.addEventListener("some-other-event", () => {});
el.removeEventListener("nodeclick", () => {});

// The default tag name is known to the DOM typings.
const created: SociogramElement = document.createElement("sociogram-view");
void created;

// Properties.
el.data = { nodeCount: 2, edges: [0, 1] };
el.linkFade = { startPx: 10, endPx: 50 };
// @ts-expect-error a bad link fade mode
el.linkFade = "sometimes";
