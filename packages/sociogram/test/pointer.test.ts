import { describe, expect, it, vi } from "vitest";
import { PointerMachine } from "../src/pointer.js";
import type { PointerConfig, PointerEngine, PointerInput } from "../src/pointer.js";

const at = (x: number, y: number, extra: Partial<PointerInput> = {}): PointerInput => ({
  x: x * 2,
  y: y * 2,
  clientX: x,
  clientY: y,
  shiftKey: false,
  altKey: false,
  ...extra,
});

function setup(config: Partial<PointerConfig> = {}) {
  const engine = {
    pick: vi.fn(() => -1),
    beginDrag: vi.fn(() => -1),
    dragTo: vi.fn(),
    endDrag: vi.fn(),
    panBy: vi.fn(),
    selectRect: vi.fn(),
    selectPolygon: vi.fn(),
    selectAt: vi.fn((): "none" | "node" | "link" => "none"),
    clearSelection: vi.fn(),
    hasSelection: vi.fn(() => false),
    toWorld: vi.fn((x: number, y: number) => [x / 10, y / 10] as const),
  } satisfies PointerEngine;
  const overlay = { rectangle: vi.fn(), polygon: vi.fn(), hide: vi.fn() };
  const hooks = { dragStart: vi.fn(), drag: vi.fn(), dragEnd: vi.fn(), click: vi.fn(), selectionChanged: vi.fn() };
  const cfg: PointerConfig = { dragNodes: true, tool: "none", clickSelection: "node", ...config };
  const machine = new PointerMachine({ engine, overlay, hooks, config: () => cfg, scale: () => 2 });
  return { engine, overlay, hooks, machine, cfg };
}

describe("PointerMachine", () => {
  it("pans the camera when the press is not on a node", () => {
    const { engine, machine } = setup();
    machine.down(at(10, 10));
    machine.move(at(30, 25));
    expect(engine.panBy).toHaveBeenCalledWith(40, 30); // CSS delta times scale 2
    expect(engine.dragTo).not.toHaveBeenCalled();
    machine.up(at(30, 25));
    expect(machine.busy).toBe(false);
  });

  it("drags the node under the pointer instead of panning, and reports world positions", () => {
    const { engine, hooks, machine } = setup();
    engine.pick.mockReturnValue(7);
    engine.beginDrag.mockReturnValue(7);
    machine.down(at(10, 10));
    expect(hooks.dragStart).not.toHaveBeenCalled(); // a press may still be a click
    machine.move(at(30, 25));
    expect(hooks.dragStart).toHaveBeenCalledWith(7, 2, 2); // world position of the press
    expect(engine.dragTo).toHaveBeenCalledWith(60, 50);
    expect(engine.panBy).not.toHaveBeenCalled();
    expect(hooks.drag).toHaveBeenCalledWith(7, 6, 5);
    machine.up(at(30, 25));
    expect(engine.endDrag).toHaveBeenCalledTimes(1);
    expect(hooks.dragEnd).toHaveBeenCalledWith(7, 6, 5);
    expect(hooks.click).not.toHaveBeenCalled();
  });

  it("never starts a node drag when dragNodes is off", () => {
    const { engine, machine } = setup({ dragNodes: false });
    engine.beginDrag.mockReturnValue(7);
    machine.down(at(10, 10));
    machine.move(at(30, 25));
    expect(engine.beginDrag).not.toHaveBeenCalled();
    expect(engine.panBy).toHaveBeenCalled();
  });

  it("treats a press without movement as a click: selects and reports the node", () => {
    const { engine, hooks, machine } = setup();
    engine.pick.mockReturnValue(3);
    engine.selectAt.mockReturnValue("node");
    machine.down(at(10, 10));
    machine.up(at(11, 10));
    expect(hooks.click).toHaveBeenCalledWith(3);
    expect(engine.selectAt).toHaveBeenCalledWith(22, 20, false, "replace");
    expect(hooks.selectionChanged).toHaveBeenCalledTimes(1);
  });

  it("uses shift to add, alt to remove, and neighbours when configured", () => {
    const { engine, machine } = setup({ clickSelection: "neighbours" });
    machine.down(at(10, 10, { shiftKey: true }));
    machine.up(at(10, 10, { shiftKey: true }));
    expect(engine.selectAt).toHaveBeenLastCalledWith(20, 20, true, "add");
    machine.down(at(10, 10, { altKey: true }));
    machine.up(at(10, 10, { altKey: true }));
    expect(engine.selectAt).toHaveBeenLastCalledWith(20, 20, true, "remove");
  });

  it("clears the selection when an empty spot is clicked, but only if something is selected", () => {
    const { engine, hooks, machine } = setup();
    machine.down(at(10, 10));
    machine.up(at(10, 10));
    expect(engine.clearSelection).not.toHaveBeenCalled();
    engine.hasSelection.mockReturnValue(true);
    machine.down(at(10, 10));
    machine.up(at(10, 10));
    expect(engine.clearSelection).toHaveBeenCalledTimes(1);
    expect(hooks.selectionChanged).toHaveBeenCalledTimes(1);
  });

  it("does not select on click when clickSelection is none, but still reports the click", () => {
    const { engine, hooks, machine } = setup({ clickSelection: "none" });
    engine.pick.mockReturnValue(3);
    machine.down(at(10, 10));
    machine.up(at(10, 10));
    expect(hooks.click).toHaveBeenCalledWith(3);
    expect(engine.selectAt).not.toHaveBeenCalled();
  });

  it("draws a rectangle and selects when the rectangle tool is active, without panning", () => {
    const { engine, overlay, hooks, machine } = setup({ tool: "rectangle" });
    machine.down(at(10, 10));
    machine.move(at(60, 40));
    expect(overlay.rectangle).toHaveBeenLastCalledWith(10, 10, 60, 40);
    expect(engine.panBy).not.toHaveBeenCalled();
    machine.up(at(60, 40, { shiftKey: true }));
    expect(overlay.hide).toHaveBeenCalled();
    expect(engine.selectRect).toHaveBeenCalledWith(20, 20, 120, 80, "add");
    expect(hooks.selectionChanged).toHaveBeenCalledTimes(1);
  });

  it("collects a lasso path and selects the polygon on release", () => {
    const { engine, overlay, machine } = setup({ tool: "lasso" });
    machine.down(at(0, 0));
    machine.move(at(10, 0));
    machine.move(at(10, 10));
    machine.up(at(0, 10));
    expect(overlay.polygon).toHaveBeenCalled();
    const [xy, op] = engine.selectPolygon.mock.calls[0] as unknown as [Float32Array, string];
    expect(Array.from(xy)).toEqual([0, 0, 20, 0, 20, 20, 0, 20]);
    expect(op).toBe("replace");
  });

  it("treats a click while a selection tool is active as an empty-area click", () => {
    const { engine, machine } = setup({ tool: "rectangle" });
    engine.hasSelection.mockReturnValue(true);
    machine.down(at(10, 10));
    machine.up(at(10, 10));
    expect(engine.selectRect).not.toHaveBeenCalled();
    expect(engine.clearSelection).toHaveBeenCalledTimes(1);
  });

  it("cancel ends a node drag and hides the overlay", () => {
    const a = setup();
    a.engine.pick.mockReturnValue(1);
    a.engine.beginDrag.mockReturnValue(1);
    a.machine.down(at(1, 1));
    a.machine.move(at(30, 30));
    a.machine.cancel();
    expect(a.engine.endDrag).toHaveBeenCalledTimes(1);
    expect(a.hooks.dragEnd).toHaveBeenCalledTimes(1);
    expect(a.machine.busy).toBe(false);

    const b = setup({ tool: "lasso" });
    b.machine.down(at(1, 1));
    b.machine.move(at(5, 5));
    b.machine.cancel();
    expect(b.overlay.hide).toHaveBeenCalled();
    expect(b.engine.selectPolygon).not.toHaveBeenCalled();
  });

  it("a press on a node without moving is a click and starts no drag", () => {
    const { engine, hooks, machine } = setup();
    engine.pick.mockReturnValue(3);
    engine.beginDrag.mockReturnValue(3);
    machine.down(at(10, 10));
    machine.up(at(11, 10));
    expect(engine.beginDrag).not.toHaveBeenCalled();
    expect(hooks.dragStart).not.toHaveBeenCalled();
    expect(hooks.dragEnd).not.toHaveBeenCalled();
    expect(hooks.click).toHaveBeenCalledWith(3);
  });

  it("stores lasso points at least 2 px apart and stops at 1000 points", () => {
    const jitter = setup({ tool: "lasso" });
    jitter.machine.down(at(0, 0));
    for (let i = 0; i < 10; i++) jitter.machine.move(at(0.5 * ((i % 2) + 1), 0));
    jitter.machine.up(at(1, 0));
    expect(jitter.engine.selectPolygon).not.toHaveBeenCalled(); // fewer than 3 stored points

    const long = setup({ tool: "lasso" });
    long.machine.down(at(0, 0));
    for (let i = 1; i <= 6000; i++) long.machine.move(at(i * 3, (i % 2) * 3));
    long.machine.up(at(9000, 0));
    const [xy] = long.engine.selectPolygon.mock.calls[0] as unknown as [Float32Array];
    expect(xy.length / 2).toBeLessThanOrEqual(1001);
  });

  it("ignores a second press while a gesture is running", () => {
    const { engine, machine } = setup();
    engine.pick.mockReturnValue(2);
    engine.beginDrag.mockReturnValue(2);
    machine.down(at(1, 1));
    machine.down(at(50, 50)); // ignored
    machine.move(at(30, 30));
    expect(engine.beginDrag).toHaveBeenCalledTimes(1);
    expect(engine.beginDrag).toHaveBeenCalledWith(2, 2); // the first press, in canvas pixels
  });
});
