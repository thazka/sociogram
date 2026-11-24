import type { PointerOverlay } from "./pointer.js";

const STROKE = "1px solid rgba(255,255,255,0.9)";

/**
 * Draws the selection rectangle or lasso above the page; never touches the
 * canvas, so it is not part of a screenshot. Styles are set through the CSSOM
 * (not `setAttribute("style")`), which a strict `style-src` policy allows.
 */
export class DomOverlay implements PointerOverlay {
  readonly #doc: Document;
  #box: HTMLDivElement | undefined;
  #svg: SVGSVGElement | undefined;

  constructor(doc: Document) {
    this.#doc = doc;
  }

  rectangle(x0: number, y0: number, x1: number, y1: number): void {
    const box = (this.#box ??= this.#makeBox());
    box.style.left = `${Math.min(x0, x1)}px`;
    box.style.top = `${Math.min(y0, y1)}px`;
    box.style.width = `${Math.abs(x1 - x0)}px`;
    box.style.height = `${Math.abs(y1 - y0)}px`;
  }

  polygon(points: ReadonlyArray<readonly [number, number]>): void {
    if (this.#svg === undefined) {
      const ns = "http://www.w3.org/2000/svg";
      const svg = this.#doc.createElementNS(ns, "svg");
      svg.style.cssText =
        "position:fixed;left:0;top:0;width:100vw;height:100vh;pointer-events:none;z-index:2147483000";
      const line = this.#doc.createElementNS(ns, "polyline");
      line.setAttribute("fill", "rgba(255,255,255,0.12)");
      line.setAttribute("stroke", "rgba(255,255,255,0.9)");
      line.setAttribute("stroke-width", "2");
      svg.appendChild(line);
      this.#doc.body.appendChild(svg);
      this.#svg = svg;
    }
    this.#svg.firstElementChild?.setAttribute("points", points.map(([x, y]) => `${x},${y}`).join(" "));
  }

  hide(): void {
    this.#box?.remove();
    this.#svg?.remove();
    this.#box = this.#svg = undefined;
  }

  #makeBox(): HTMLDivElement {
    const el = this.#doc.createElement("div");
    el.style.cssText = `position:fixed;pointer-events:none;z-index:2147483000;border:${STROKE};background:rgba(255,255,255,0.12);box-sizing:border-box`;
    this.#doc.body.appendChild(el);
    return el;
  }
}
