export type StreamDeltaBatch = {
  text: string;
  reasoningText: string;
};

export type StreamDeltaBuffer = {
  appendText: (text: string) => void;
  appendReasoning: (text: string) => void;
  flush: () => void;
  clear: () => void;
};

export function createStreamDeltaBuffer(
  onFlush: (batch: StreamDeltaBatch) => void,
): StreamDeltaBuffer {
  let frame: number | null = null;
  let text = "";
  let reasoningText = "";

  function emit() {
    if (!text && !reasoningText) {
      return;
    }
    const batch = { text, reasoningText };
    text = "";
    reasoningText = "";
    onFlush(batch);
  }

  function schedule() {
    if (frame != null) {
      return;
    }
    const scheduledFrame = requestAnimationFrame(() => {
      if (frame !== scheduledFrame) {
        return;
      }
      frame = null;
      emit();
    });
    frame = scheduledFrame;
  }

  function cancelScheduledFrame() {
    if (frame != null) {
      cancelAnimationFrame(frame);
      frame = null;
    }
  }

  return {
    appendText(delta) {
      text += delta;
      schedule();
    },
    appendReasoning(delta) {
      reasoningText += delta;
      schedule();
    },
    flush() {
      cancelScheduledFrame();
      emit();
    },
    clear() {
      cancelScheduledFrame();
      text = "";
      reasoningText = "";
    },
  };
}
