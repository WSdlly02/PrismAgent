import { describe, expect, it } from "vitest";
import { approvalMaskForAll, approvalMaskForManual } from "./approval";

describe("approval masks", () => {
  it("denies all with zero", () => {
    expect(approvalMaskForAll(0)).toBe(0);
  });

  it("approves all requested tool calls with a bit per call", () => {
    expect(approvalMaskForAll(1)).toBe(1);
    expect(approvalMaskForAll(3)).toBe(7);
    expect(approvalMaskForAll(5)).toBe(31);
  });

  it("caps browser-safe approval masks to 63 tool calls", () => {
    expect(approvalMaskForAll(64)).toBe(Number.MAX_SAFE_INTEGER);
  });

  it("approves only the backend supplied manual approval mask", () => {
    expect(approvalMaskForManual(0b101)).toBe(0b101);
  });
});
