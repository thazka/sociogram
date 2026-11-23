<script setup lang="ts">
import { ref, shallowRef } from "vue";
import { Sociogram } from "sociogram/vue";
import type { SociogramHandle } from "sociogram/vue";
import { communityGraph } from "./graph";

// A shallowRef: the graph holds large typed arrays that Vue does not need to track.
const graph = shallowRef(communityGraph());
const handle = ref<SociogramHandle | null>(null);
const shown = ref(true);
const status = ref("loading");
const clicks = ref(0);
const hover = ref<number | null>(null);

function toggle() {
  status.value = "loading"; // a new mount starts from scratch
  shown.value = !shown.value;
}
</script>

<template>
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
</template>
