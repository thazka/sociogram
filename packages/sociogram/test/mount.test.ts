import { describe, expect, it, vi } from "vitest";
import { mountSociogram } from "../src/mount.js";
import type { MountCallbacks, MountDeps } from "../src/mount.js";
import type { GraphData, SociogramView } from "../src/view.js";

// ---- fakes: no browser, no GPU. The helper only needs a canvas it can remove
// and a view with a handful of methods.

type Listener = (...args: never[]) => void;

function fakeView() {
  const listeners = new Map<string, Set<Listener>>();
  const view = {
    setData: vi.fn<(d: GraphData) => void>(),
    setLinkFade: vi.fn<(m: unknown) => void>(),
    destroy: vi.fn<() => void>(),
    on: vi.fn((event: string, fn: Listener) => {
      let set = listeners.get(event);
      if (set === undefined) listeners.set(event, (set = new Set()));
      set.add(fn);
      return () => void set.delete(fn);
    }),
  };
  return {
    view,
    asView: view as unknown as SociogramView,
    emit(event: string, ...args: unknown[]) {
      for (const fn of listeners.get(event) ?? []) (fn as (...a: unknown[]) => void)(...args);
    },
    listenerCount: (event: string) => listeners.get(event)?.size ?? 0,
  };
}

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function setup() {
  const canvas = { remove: vi.fn() } as unknown as HTMLCanvasElement;
  const pending = deferred<SociogramView>();
  const create = vi.fn<MountDeps["create"]>(() => pending.promise);
  const deps: MountDeps = { create, createCanvas: () => canvas };
  const container = {} as HTMLElement;
  return { canvas, pending, create, deps, container };
}

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

const graph = (n = 2): GraphData => ({ nodeCount: n, edges: [0, 1] });

describe("mountSociogram", () => {
  it("creates the view once and applies the initial props when it is ready", async () => {
    const { deps, container, pending, create } = setup();
    const onReady = vi.fn();
    const data = graph();
    mountSociogram(container, { initial: { data, linkFade: "off" }, callbacks: () => ({ onReady }) }, deps);
    expect(create).toHaveBeenCalledTimes(1);

    const f = fakeView();
    pending.resolve(f.asView);
    await flush();

    expect(f.view.setData).toHaveBeenCalledExactlyOnceWith(data);
    expect(f.view.setLinkFade).toHaveBeenCalledExactlyOnceWith("off");
    expect(onReady).toHaveBeenCalledExactlyOnceWith(f.asView);
  });

  it("passes the create options through", () => {
    const { deps, container, create } = setup();
    mountSociogram(container, { options: { maxPixelRatio: 2 } }, deps);
    expect(create.mock.calls[0]?.[1]).toEqual({ maxPixelRatio: 2 });
  });

  it("keeps only the latest props that arrive before the view is ready", async () => {
    const { deps, container, pending } = setup();
    const m = mountSociogram(container, {}, deps);
    const first = graph(1);
    const second = graph(2);
    m.update({ data: first, linkFade: "off" });
    m.update({ data: second });

    const f = fakeView();
    pending.resolve(f.asView);
    await flush();

    expect(f.view.setData).toHaveBeenCalledExactlyOnceWith(second);
    expect(f.view.setLinkFade).toHaveBeenCalledExactlyOnceWith("off");
  });

  it("does not set the same data twice but does set a new object", async () => {
    const { deps, container, pending } = setup();
    const m = mountSociogram(container, {}, deps);
    const f = fakeView();
    pending.resolve(f.asView);
    await flush();

    const data = graph();
    m.update({ data });
    m.update({ data });
    m.update({ data, linkFade: "auto" });
    expect(f.view.setData).toHaveBeenCalledTimes(1);

    m.update({ data: graph() });
    expect(f.view.setData).toHaveBeenCalledTimes(2);
  });

  it("applies an unchanged link fade only once and a changed one again", async () => {
    const { deps, container, pending } = setup();
    const m = mountSociogram(container, {}, deps);
    const f = fakeView();
    pending.resolve(f.asView);
    await flush();

    m.update({ linkFade: { startPx: 10, endPx: 50 } });
    m.update({ linkFade: { startPx: 10, endPx: 50 } }); // equal value, new object
    expect(f.view.setLinkFade).toHaveBeenCalledTimes(1);
    m.update({ linkFade: { startPx: 10, endPx: 80 } });
    m.update({ linkFade: "auto" });
    expect(f.view.setLinkFade).toHaveBeenCalledTimes(3);
  });

  it("gives the GPU back when it is destroyed before the view is ready (unmount race)", async () => {
    const { deps, container, pending, canvas } = setup();
    const onReady = vi.fn();
    const onError = vi.fn();
    const m = mountSociogram(container, { callbacks: () => ({ onReady, onError }), initial: { data: graph() } }, deps);

    m.destroy();
    expect(canvas.remove).toHaveBeenCalledTimes(1);

    const f = fakeView();
    pending.resolve(f.asView);
    await flush();

    expect(f.view.destroy).toHaveBeenCalledTimes(1); // the late view is destroyed, not leaked
    expect(f.view.setData).not.toHaveBeenCalled();
    expect(onReady).not.toHaveBeenCalled();
    expect(onError).not.toHaveBeenCalled();
    expect(m.view).toBeUndefined();
  });

  it("survives React StrictMode: mount, destroy, mount again, both creations resolve", async () => {
    const first = setup();
    const m1 = mountSociogram(first.container, { initial: { data: graph() } }, first.deps);
    m1.destroy();
    const second = setup();
    const onReady = vi.fn();
    const m2 = mountSociogram(second.container, { initial: { data: graph() }, callbacks: () => ({ onReady }) }, second.deps);

    const late = fakeView();
    const live = fakeView();
    first.pending.resolve(late.asView);
    second.pending.resolve(live.asView);
    await flush();

    expect(late.view.destroy).toHaveBeenCalledTimes(1);
    expect(live.view.destroy).not.toHaveBeenCalled();
    expect(live.view.setData).toHaveBeenCalledTimes(1);
    expect(onReady).toHaveBeenCalledTimes(1);
    expect(m2.view).toBe(live.asView);
  });

  it("destroys the view and removes the canvas once, and ignores everything afterwards", async () => {
    const { deps, container, pending, canvas } = setup();
    const m = mountSociogram(container, {}, deps);
    const f = fakeView();
    pending.resolve(f.asView);
    await flush();

    m.destroy();
    m.destroy();
    m.update({ data: graph() });
    expect(f.view.destroy).toHaveBeenCalledTimes(1);
    expect(canvas.remove).toHaveBeenCalledTimes(1);
    expect(f.view.setData).not.toHaveBeenCalled();
    expect(m.view).toBeUndefined();
  });

  it("unsubscribes from view events on destroy", async () => {
    const { deps, container, pending } = setup();
    const onNodeClick = vi.fn();
    const m = mountSociogram(container, { callbacks: () => ({ onNodeClick }) }, deps);
    const f = fakeView();
    pending.resolve(f.asView);
    await flush();
    expect(f.listenerCount("nodeClick")).toBe(1);

    m.destroy();
    expect(f.listenerCount("nodeClick")).toBe(0);
    expect(f.listenerCount("nodeHover")).toBe(0);
    expect(f.listenerCount("error")).toBe(0);
  });

  it("forwards events to the latest callbacks without remounting", async () => {
    const { deps, container, pending } = setup();
    let current: MountCallbacks = {};
    mountSociogram(container, { callbacks: () => current }, deps);
    const f = fakeView();
    pending.resolve(f.asView);
    await flush();

    const first = vi.fn();
    const second = vi.fn();
    current = { onNodeClick: first };
    f.emit("nodeClick", 3);
    current = { onNodeClick: second, onNodeHover: vi.fn() };
    f.emit("nodeClick", 5);

    expect(first).toHaveBeenCalledExactlyOnceWith(3);
    expect(second).toHaveBeenCalledExactlyOnceWith(5);
  });

  it("reports a failed creation through onError instead of throwing", async () => {
    const { deps, container, pending } = setup();
    const onError = vi.fn();
    const m = mountSociogram(container, { callbacks: () => ({ onError }) }, deps);
    const failure = new Error("no GPU");
    pending.reject(failure);
    await flush();

    expect(onError).toHaveBeenCalledExactlyOnceWith(failure, "create");
    expect(() => {
      m.update({ data: graph() });
      m.destroy();
    }).not.toThrow();
  });

  it("wraps non-Error rejections", async () => {
    const { deps, container, pending } = setup();
    const onError = vi.fn();
    mountSociogram(container, { callbacks: () => ({ onError }) }, deps);
    pending.reject("plain string");
    await flush();
    const arg = onError.mock.calls[0]?.[0];
    expect(arg).toBeInstanceOf(Error);
    expect((arg as Error).message).toBe("plain string");
  });

  it("reports data the engine rejects, keeps working, and does not retry the same object", async () => {
    const { deps, container, pending } = setup();
    const onError = vi.fn();
    const m = mountSociogram(container, { callbacks: () => ({ onError }) }, deps);
    const f = fakeView();
    f.view.setData.mockImplementationOnce(() => {
      throw new Error("invalid graph: edge 0 references node 9");
    });
    pending.resolve(f.asView);
    await flush();

    const bad = graph();
    m.update({ data: bad });
    expect(onError).toHaveBeenCalledTimes(1);
    expect(onError.mock.calls[0]?.[1]).toBe("data");
    m.update({ data: bad });
    expect(f.view.setData).toHaveBeenCalledTimes(1); // not retried in a loop

    const good = graph(3);
    m.update({ data: good });
    expect(f.view.setData).toHaveBeenLastCalledWith(good);
    expect(onError).toHaveBeenCalledTimes(1);
  });

  it("reports an invalid link fade range", async () => {
    const { deps, container, pending } = setup();
    const onError = vi.fn();
    const m = mountSociogram(container, { callbacks: () => ({ onError }) }, deps);
    const f = fakeView();
    f.view.setLinkFade.mockImplementation(() => {
      throw new RangeError("setLinkFade: expected finite 0 <= startPx < endPx");
    });
    pending.resolve(f.asView);
    await flush();

    m.update({ linkFade: { startPx: 9, endPx: 1 } });
    expect(onError).toHaveBeenCalledTimes(1);
    expect(onError.mock.calls[0]?.[0]).toBeInstanceOf(RangeError);
    expect(onError.mock.calls[0]?.[1]).toBe("data");
  });

  it("forwards render-loop errors from the view", async () => {
    const { deps, container, pending } = setup();
    const onError = vi.fn();
    mountSociogram(container, { callbacks: () => ({ onError }) }, deps);
    const f = fakeView();
    pending.resolve(f.asView);
    await flush();

    const failure = new Error("device lost");
    f.emit("error", failure);
    expect(onError).toHaveBeenCalledExactlyOnceWith(failure, "render");
  });

  it("a throwing onReady does not break the mount", async () => {
    const { deps, container, pending } = setup();
    const onError = vi.fn();
    const m = mountSociogram(
      container,
      {
        callbacks: () => ({
          onError,
          onReady: () => {
            throw new Error("host bug");
          },
        }),
      },
      deps,
    );
    const f = fakeView();
    pending.resolve(f.asView);
    await flush();

    // The host's bug is surfaced (as its own kind), and the view stays usable.
    expect(onError).toHaveBeenCalledTimes(1);
    expect(onError.mock.calls[0]?.[1]).toBe("callback");
    expect(m.view).toBe(f.asView);
    expect(() => m.destroy()).not.toThrow();
  });
});
