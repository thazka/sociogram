<script setup lang="ts">
import { ref, shallowRef } from "vue";
import { Sociogram } from "sociogram/vue";
import type { SociogramHandle } from "sociogram/vue";
import { communityGraph } from "./graph";

// Rendered on the server first: the component renders an empty container there
// and creates the view only in the browser, so no <ClientOnly> wrapper is needed.
const graph = shallowRef(communityGraph());
const handle = ref<SociogramHandle | null>(null);
const shown = ref(true);
const status = ref("loading");
const clicks = ref(0);
const hover = ref<number | null>(null);

function toggle() {
  status.value = "loading";
  shown.value = !shown.value;
}
</script>

<template>
  <main>
    <h1>sociogram + Nuxt</h1>
    <p>
      <button id="toggle" @click="toggle">{{ shown ? "Unmount" : "Mount" }}</button>
      <button id="fit" @click="handle?.fitView()">Fit</button>
      <button id="reheat" @click="handle?.reheat()">Reheat</button>
      <span id="status" :data-status="shown ? status : 'unmounted'">{{ shown ? status : "unmounted" }}</span>
      clicks: <span id="clicks">{{ clicks }}</span> hover: <span id="hover">{{ hover ?? "none" }}</span>
    </p>
    <div v-if="shown" class="graph">
      <Sociogram
        ref="handle"
        :data="graph"
        @ready="status = 'ready'"
        @error="(error, phase) => (status = `error (${phase}): ${error.message}`)"
        @node-click="clicks++"
        @node-hover="(i) => (hover = i)"
      >
        <template #fallback="{ error }">
          <p style="padding: 16px">WebGPU is not available: {{ error.message }}</p>
        </template>
      </Sociogram>
    </div>
  </main>
</template>

<style>
html, body { margin: 0; background: #0b0e13; color: #c9d1d9; font: 14px system-ui, sans-serif; }
main { padding: 12px; }
.graph { height: 480px; border: 1px solid #30363d; border-radius: 8px; overflow: hidden; }
button { margin-right: 8px; }
</style>
