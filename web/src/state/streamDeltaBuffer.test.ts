import { describe, expect, it, vi } from "vitest";
import { createStreamDeltaBuffer } from "./streamDeltaBuffer";

describe("createStreamDeltaBuffer", () => {
  it("batches text and reasoning deltas into one animation frame", () => {
    const frames = new Map<number, FrameRequestCallback>();
    let nextFrame = 1;
    vi.spyOn(globalThis, "requestAnimationFrame").mockImplementation(
      (callback) => {
        const frame = nextFrame++;
        frames.set(frame, callback);
        return frame;
      },
    );
    const onFlush = vi.fn();
    const buffer = createStreamDeltaBuffer(onFlush);

    buffer.appendText("hel");
    buffer.appendReasoning("think");
    buffer.appendText("lo");

    expect(requestAnimationFrame).toHaveBeenCalledTimes(1);
    expect(onFlush).not.toHaveBeenCalled();

    frames.get(1)?.(16);
    expect(onFlush).toHaveBeenCalledWith({
      text: "hello",
      reasoningText: "think",
    });
  });

  it("does not flush a cancelled frame after the buffer is cleared", () => {
    const frames = new Map<number, FrameRequestCallback>();
    vi.spyOn(globalThis, "requestAnimationFrame").mockImplementation(
      (callback) => {
        frames.set(1, callback);
        return 1;
      },
    );
    const cancelFrame = vi.spyOn(globalThis, "cancelAnimationFrame");
    const onFlush = vi.fn();
    const buffer = createStreamDeltaBuffer(onFlush);

    buffer.appendText("stale");
    buffer.clear();
    frames.get(1)?.(16);

    expect(cancelFrame).toHaveBeenCalledWith(1);
    expect(onFlush).not.toHaveBeenCalled();
  });
});
