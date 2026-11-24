import type { PointerEngine, SelectOp } from "./pointer.js";
import type { Sociogram } from "./wasm/sociogram_web.js";

/** Operation codes understood by the wasm engine. */
export const OP_CODE: Record<SelectOp, number> = { replace: 0, add: 1, remove: 2 };
const HITS = ["none", "node", "link"] as const;

/** Adapts the wasm engine to the small interface the pointer state machine needs. */
export function adaptEngine(engine: Sociogram): PointerEngine {
  return {
    pick: (x, y) => engine.pick(x, y),
    beginDrag: (x, y) => engine.begin_drag(x, y),
    dragTo: (x, y) => engine.drag_to(x, y),
    endDrag: () => engine.end_drag(),
    panBy: (dx, dy) => engine.pan_by(dx, dy),
    selectRect: (x0, y0, x1, y1, op) => engine.select_rect(x0, y0, x1, y1, OP_CODE[op]),
    selectPolygon: (xy, op) => engine.select_polygon(xy, OP_CODE[op]),
    selectAt: (x, y, neighbours, op) => HITS[engine.select_at(x, y, neighbours, OP_CODE[op])] ?? "none",
    clearSelection: () => engine.clear_selection(),
    hasSelection: () => engine.selection_active(),
    toWorld: (x, y) => {
      const w = engine.screen_to_world(x, y);
      return [w[0] as number, w[1] as number] as const;
    },
  };
}
