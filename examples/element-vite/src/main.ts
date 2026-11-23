import { defineSociogramElement } from "sociogram/element";
import type { SociogramElement } from "sociogram/element";
import { communityGraph } from "./graph";

defineSociogramElement(); // registers <sociogram-view>, once

const graph = communityGraph();
const host = document.getElementById("host")!;
const toggle = document.getElementById("toggle")!;
const status = document.getElementById("status")!;
const clicks = document.getElementById("clicks")!;
const hover = document.getElementById("hover")!;

let element: SociogramElement | undefined;
let clickCount = 0;

function setStatus(text: string): void {
  status.textContent = text;
  status.dataset["status"] = text.startsWith("error") ? "error" : text;
}

function mount(): void {
  setStatus("loading");
  // The tag name is known to TypeScript, and so are the event types: no casts.
  const el = document.createElement("sociogram-view");
  el.data = graph; // properties, not attributes: the graph holds large typed arrays
  el.addEventListener("ready", () => setStatus("ready"));
  el.addEventListener("nodeclick", () => {
    clickCount += 1;
    clicks.textContent = String(clickCount);
  });
  el.addEventListener("nodehover", (e) => {
    hover.textContent = e.detail === null ? "none" : String(e.detail);
  });
  el.addEventListener("error", (e) => {
    setStatus(`error (${e.detail.phase}): ${e.detail.error.message}`);
  });
  host.append(el);
  element = el;
  toggle.textContent = "Unmount";
}

function unmount(): void {
  element?.remove(); // disconnecting the element releases the GPU
  element = undefined;
  setStatus("unmounted");
  toggle.textContent = "Mount";
}

toggle.addEventListener("click", () => (element ? unmount() : mount()));
mount();
