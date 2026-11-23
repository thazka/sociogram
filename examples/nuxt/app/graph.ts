import type { GraphData } from "sociogram";

const PALETTE = [
  [0.96, 0.45, 0.45],
  [0.45, 0.8, 0.55],
  [0.45, 0.6, 0.97],
  [0.97, 0.8, 0.4],
  [0.75, 0.55, 0.95],
] as const;

/** A graph made of dense communities joined by a few long links. */
export function communityGraph(groups = 5, perGroup = 80): GraphData {
  let seed = 12345;
  const random = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 4294967296);
  const n = groups * perGroup;
  const edges: number[] = [];
  for (let i = 0; i < n; i++) {
    const start = Math.floor(i / perGroup) * perGroup;
    for (let k = 0; k < 3; k++) edges.push(i, start + Math.floor(random() * perGroup));
  }
  for (let k = 0; k < 30; k++) edges.push(Math.floor(random() * n), Math.floor(random() * n));
  const colors = new Float32Array(n * 4);
  for (let i = 0; i < n; i++) {
    const [r, g, b] = PALETTE[Math.floor(i / perGroup) % PALETTE.length]!;
    colors.set([r, g, b, 1], i * 4);
  }
  return { nodeCount: n, edges: new Uint32Array(edges), colors };
}
