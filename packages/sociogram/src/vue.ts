// Vue 3 wrapper: `import { Sociogram } from "sociogram/vue"`.
//
// Works in Vue 3 and Nuxt (server rendering produces an empty container; the
// view is created in `onMounted`, which only runs in the browser).
//
//   <Sociogram :data="graph" @node-click="onClick" style="height: 400px">
//     <template #fallback="{ error }">WebGPU is not available: {{ error.message }}</template>
//   </Sociogram>
//
// Keep the graph in a `shallowRef` or `markRaw` it: it can hold large typed
// arrays that Vue does not need to track.

import { defineComponent, h, onBeforeUnmount, onMounted, shallowRef, toRaw, watch } from "vue";
import type { PropType } from "vue";
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

/** What a template `ref` to the component exposes (type it as `ref<SociogramHandle | null>`). */
export interface SociogramHandle {
  /** The underlying view once ready, otherwise `undefined`. */
  getView(): SociogramView | undefined;
  fitView(): void;
  reheat(): void;
  /** Rejects when the view is not ready. */
  screenshot(): Promise<Blob>;
}

const fill = { position: "absolute", top: "0", right: "0", bottom: "0", left: "0" } as const;

export const Sociogram = defineComponent({
  name: "Sociogram",
  props: {
    /** The graph. Pass a new object to replace it; the same object is applied once. */
    data: { type: Object as PropType<GraphData>, default: undefined },
    linkFade: { type: [String, Object] as PropType<LinkFadeMode>, default: undefined },
    dragNodes: { type: Boolean, default: undefined },
    /** Keep the array reference stable: a new array re-applies the pins. */
    pinnedNodes: { type: [Array, Object] as PropType<ArrayLike<number>>, default: undefined },
    selectionTool: { type: String as PropType<SelectionTool>, default: undefined },
    /** Create-time options. Changing `wasmUrl` or `maxPixelRatio` remounts the view. */
    options: { type: Object as PropType<SociogramOptions>, default: undefined },
  },
  emits: {
    ready: (_view: SociogramView) => true,
    error: (_error: Error, _phase: MountErrorPhase) => true,
    nodeHover: (_index: number | null) => true,
    nodeClick: (_index: number) => true,
    nodeDragStart: (_e: NodeDragEvent) => true,
    nodeDrag: (_e: NodeDragEvent) => true,
    nodeDragEnd: (_e: NodeDragEvent) => true,
    selectionChange: (_e: SelectionChange) => true,
  },
  setup(props, { emit, expose, slots }) {
    const host = shallowRef<HTMLElement | null>(null);
    const failure = shallowRef<Error | undefined>(undefined);
    let mounted: Mounted | undefined;

    const start = (): void => {
      const element = host.value;
      if (element === null) return;
      failure.value = undefined;
      const options: SociogramOptions = {};
      const wasmUrl = props.options?.wasmUrl;
      const maxPixelRatio = props.options?.maxPixelRatio;
      if (wasmUrl !== undefined) options.wasmUrl = wasmUrl;
      if (maxPixelRatio !== undefined) options.maxPixelRatio = maxPixelRatio;
      // Create-time only: changing these later does not remount the view.
      const given = props.options;
      if (given?.dragHeat !== undefined) options.dragHeat = given.dragHeat;
      if (given?.clickSelection !== undefined) options.clickSelection = given.clickSelection;
      if (given?.dimUnselected !== undefined) options.dimUnselected = given.dimUnselected;
      mounted = mountSociogram(element, {
        options,
        initial: {
          data: raw(props.data),
          linkFade: raw(props.linkFade),
          dragNodes: props.dragNodes,
          pinnedNodes: raw(props.pinnedNodes),
          selectionTool: props.selectionTool,
        },
        callbacks: () => ({
          onReady: (view) => emit("ready", view),
          onNodeHover: (i) => emit("nodeHover", i),
          onNodeClick: (i) => emit("nodeClick", i),
          onNodeDragStart: (e) => emit("nodeDragStart", e),
          onNodeDrag: (e) => emit("nodeDrag", e),
          onNodeDragEnd: (e) => emit("nodeDragEnd", e),
          onSelectionChange: (e) => emit("selectionChange", e),
          onError: (error, phase) => {
            if (phase === "create") failure.value = error;
            emit("error", error, phase);
          },
        }),
      });
    };
    const stop = (): void => {
      mounted?.destroy();
      mounted = undefined;
    };

    onMounted(start);
    onBeforeUnmount(stop);

    watch(
      () => props.data,
      (data) => mounted?.update({ data: raw(data) }),
    );
    watch(
      () => props.linkFade,
      (linkFade) => mounted?.update({ linkFade: raw(linkFade) }),
    );
    watch(
      () => props.dragNodes,
      (dragNodes) => mounted?.update({ dragNodes }),
    );
    watch(
      () => props.pinnedNodes,
      (pinnedNodes) => mounted?.update({ pinnedNodes: raw(pinnedNodes) }),
    );
    watch(
      () => props.selectionTool,
      (selectionTool) => mounted?.update({ selectionTool }),
    );
    watch(
      () => [props.options?.wasmUrl, props.options?.maxPixelRatio] as const,
      () => {
        stop();
        start();
      },
    );

    expose({
      /** The underlying view once ready, otherwise `undefined`. */
      getView: (): SociogramView | undefined => mounted?.view,
      fitView: (): void => mounted?.view?.fitView(),
      reheat: (): void => mounted?.view?.reheat(),
      /** Rejects when the view is not ready. */
      screenshot: (): Promise<Blob> => {
        const view = mounted?.view;
        return view === undefined ? Promise.reject(new Error("Sociogram is not ready yet")) : view.screenshot();
      },
    });

    return () =>
      h("div", { style: { position: "relative", width: "100%", height: "100%" } }, [
        h("div", { ref: host, style: fill }),
        failure.value !== undefined && slots["fallback"] !== undefined
          ? h("div", { style: fill }, slots["fallback"]({ error: failure.value }))
          : null,
      ]);
  },
});

/** Props can arrive wrapped in Vue reactivity; the engine wants the plain objects. */
function raw<T>(value: T | undefined): T | undefined {
  return value === undefined ? undefined : toRaw(value);
}
