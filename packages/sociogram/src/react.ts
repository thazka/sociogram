"use client";

// React wrapper: `import { Sociogram } from "sociogram/react"`.
//
// - Works with React 18 and 19, in Next.js (App Router and Pages Router) and
//   with StrictMode. The marker above makes it a client component; nothing
//   touches the browser while rendering on the server.
// - The component renders two divs. The view lives in a canvas created and
//   removed by the lifecycle helper, so React never renders into it.

import { createElement, forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { mountSociogram } from "./mount.js";
import type { Mounted, MountErrorPhase } from "./mount.js";
import type { SelectionTool } from "./pointer.js";
import type {
  GraphData,
  LinkFadeMode,
  NodeDragEvent,
  SelectionChange,
  SociogramOptions,
  SociogramView,
} from "./view.js";

export interface SociogramProps {
  /** The graph. Pass a new object to replace it; the same object is applied once. */
  data?: GraphData;
  linkFade?: LinkFadeMode;
  /** Create-time options. Changing `wasmUrl` or `maxPixelRatio` remounts the view. */
  options?: SociogramOptions;
  /** The view is ready; use it for imperative calls. */
  onReady?: (view: SociogramView) => void;
  /** Creation failed, data was rejected, or rendering stopped. `phase` tells which. */
  onError?: (error: Error, phase: MountErrorPhase) => void;
  onNodeHover?: (index: number | null) => void;
  onNodeClick?: (index: number) => void;
  /** Drag nodes with the pointer. Default `true`. */
  dragNodes?: boolean;
  /** Indices of pinned nodes. Keep the array reference stable (state or `useMemo`): a new array re-applies the pins. */
  pinnedNodes?: ArrayLike<number>;
  /** Rectangle or lasso selection tool. `"none"` (default) leaves the pointer for dragging and panning. */
  selectionTool?: SelectionTool;
  onSelectionChange?: (e: SelectionChange) => void;
  onNodeDragStart?: (e: NodeDragEvent) => void;
  onNodeDrag?: (e: NodeDragEvent) => void;
  onNodeDragEnd?: (e: NodeDragEvent) => void;
  className?: string;
  /** The component fills its parent by default: give the parent a height. */
  style?: CSSProperties;
  /** Shown instead of the graph when the view cannot be created (for example no WebGPU). */
  fallback?: ReactNode | ((error: Error) => ReactNode);
}

/** Imperative handle, available through `ref`. */
export interface SociogramHandle {
  /** The underlying view once ready, otherwise `undefined`. */
  readonly view: SociogramView | undefined;
  fitView(): void;
  reheat(): void;
  /** Rejects when the view is not ready. */
  screenshot(): Promise<Blob>;
}

const fill: CSSProperties = { position: "absolute", top: 0, right: 0, bottom: 0, left: 0 };

export const Sociogram = forwardRef<SociogramHandle, SociogramProps>(function Sociogram(props, ref) {
  const { data, linkFade, dragNodes, pinnedNodes, selectionTool, className, style, fallback } = props;
  const wasmUrl = props.options?.wasmUrl;
  const maxPixelRatio = props.options?.maxPixelRatio;

  const hostRef = useRef<HTMLDivElement>(null);
  const mountedRef = useRef<Mounted | undefined>(undefined);
  // The latest props, read when an event fires, so new closures never remount.
  const latest = useRef(props);
  latest.current = props;
  const [failure, setFailure] = useState<Error | undefined>(undefined);

  useEffect(() => {
    const host = hostRef.current;
    if (host === null) return;
    setFailure(undefined);
    const options: SociogramOptions = {};
    const given = latest.current.options;
    if (wasmUrl !== undefined) options.wasmUrl = wasmUrl;
    if (maxPixelRatio !== undefined) options.maxPixelRatio = maxPixelRatio;
    // Create-time only: changing these later does not remount the view.
    if (given?.dragHeat !== undefined) options.dragHeat = given.dragHeat;
    if (given?.clickSelection !== undefined) options.clickSelection = given.clickSelection;
    if (given?.dimUnselected !== undefined) options.dimUnselected = given.dimUnselected;
    const mounted = mountSociogram(host, {
      options,
      initial: {
        data: latest.current.data,
        linkFade: latest.current.linkFade,
        dragNodes: latest.current.dragNodes,
        pinnedNodes: latest.current.pinnedNodes,
        selectionTool: latest.current.selectionTool,
      },
      callbacks: () => ({
        onReady: (view) => latest.current.onReady?.(view),
        onNodeHover: (i) => latest.current.onNodeHover?.(i),
        onNodeClick: (i) => latest.current.onNodeClick?.(i),
        onNodeDragStart: (e) => latest.current.onNodeDragStart?.(e),
        onNodeDrag: (e) => latest.current.onNodeDrag?.(e),
        onNodeDragEnd: (e) => latest.current.onNodeDragEnd?.(e),
        onSelectionChange: (e) => latest.current.onSelectionChange?.(e),
        onError: (error, phase) => {
          if (phase === "create") setFailure(error);
          latest.current.onError?.(error, phase);
        },
      }),
    });
    mountedRef.current = mounted;
    return () => {
      mounted.destroy();
      mountedRef.current = undefined;
    };
  }, [wasmUrl, maxPixelRatio]);

  useEffect(() => {
    mountedRef.current?.update({ data, linkFade, dragNodes, pinnedNodes, selectionTool });
  }, [data, linkFade, dragNodes, pinnedNodes, selectionTool]);

  useImperativeHandle(
    ref,
    () => ({
      get view() {
        return mountedRef.current?.view;
      },
      fitView: () => mountedRef.current?.view?.fitView(),
      reheat: () => mountedRef.current?.view?.reheat(),
      screenshot: () => {
        const view = mountedRef.current?.view;
        return view === undefined ? Promise.reject(new Error("Sociogram is not ready yet")) : view.screenshot();
      },
    }),
    [],
  );

  const showFallback = failure !== undefined && fallback !== undefined;
  return createElement(
    "div",
    { className, style: { position: "relative", width: "100%", height: "100%", ...style } },
    createElement("div", { ref: hostRef, style: fill }),
    showFallback
      ? createElement("div", { style: fill }, typeof fallback === "function" ? fallback(failure) : fallback)
      : null,
  );
});
