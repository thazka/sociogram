/**
 * Convert user input to a `Uint32Array`, rejecting (with `RangeError`) anything
 * that is not an integer below `count`. `Uint32Array.from` would silently
 * truncate `1.5` to `1` and wrap `-1`, selecting the wrong node without an error.
 */
export function toIndices(values: ArrayLike<number>, count = 0x1_0000_0000): Uint32Array {
  const out = new Uint32Array(values.length);
  for (let i = 0; i < values.length; i++) {
    const v = values[i] as number;
    if (!Number.isInteger(v) || v < 0 || v >= count) {
      throw new RangeError(`entry ${i} is not a valid node index: ${String(v)}`);
    }
    out[i] = v;
  }
  return out;
}
