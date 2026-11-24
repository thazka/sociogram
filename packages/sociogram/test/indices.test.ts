import { describe, expect, it } from "vitest";
import { toIndices } from "../src/indices.js";

describe("toIndices", () => {
  it("accepts integers, arrays and typed arrays", () => {
    expect(Array.from(toIndices([0, 5, 4294967295]))).toEqual([0, 5, 4294967295]);
    expect(Array.from(toIndices(new Uint32Array([1, 2])))).toEqual([1, 2]);
    expect(toIndices([]).length).toBe(0);
  });
  it("rejects fractions, negatives, NaN, Infinity and values above 2^32 - 1", () => {
    for (const bad of [1.5, -1, Number.NaN, Number.POSITIVE_INFINITY, 4294967296]) {
      expect(() => toIndices([0, bad])).toThrow(RangeError);
    }
  });
  it("rejects indices at or above the given node count", () => {
    expect(Array.from(toIndices([0, 4], 5))).toEqual([0, 4]);
    expect(() => toIndices([5], 5)).toThrow(RangeError);
  });
});
