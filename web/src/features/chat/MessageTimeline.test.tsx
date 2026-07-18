import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Unit } from "../../api/types";
import { MessageTimeline } from "./MessageTimeline";

const baseUnit: Unit = {
  uuid: "unit-1",
  visibility: "public",
  content: {
    role: "assistant",
    content: [
      {
        ReasoningContent: "historical reasoning should not be rendered",
      },
      { Text: "final answer" },
    ],
  },
  token_usage: null,
  metadata: {},
  created_at: 1,
};

const userUnit: Unit = {
  ...baseUnit,
  uuid: "unit-user",
  content: { role: "user", content: [{ Text: "Where are we?" }] },
};

function nextAnimationFrame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()));
}

describe("MessageTimeline", () => {
  it("does not render snapshot reasoning content after inference is committed", () => {
    const { container } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[baseUnit]}
      />,
    );

    const reasoning = container.querySelector('[data-role="reasoning"]');
    const assistant = container.querySelector('[data-role="assistant"]');

    expect(reasoning).toBeNull();
    expect(assistant?.textContent).toContain("final answer");
  });

  it("copies raw text from committed user and assistant messages", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[userUnit, baseUnit]}
      />,
    );

    const copyButtons = screen.getAllByRole("button", {
      name: "Copy message",
    });
    expect(copyButtons).toHaveLength(2);

    fireEvent.click(copyButtons[0]);

    await waitFor(() => expect(writeText).toHaveBeenCalledWith("Where are we?"));
    expect(screen.getByRole("button", { name: "Copied" })).toBeTruthy();
  });

  it("does not offer copying for an unfinished streaming message", () => {
    render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText="partial answer"
        units={[]}
      />,
    );

    expect(
      screen.queryByRole("button", { name: "Copy message" }),
    ).toBeNull();
  });

  it("renders streaming reasoning verbatim separately from streaming answer text", () => {
    const { container } = render(
      <MessageTimeline
        streamingReasoningText="private live reasoning without pub tags"
        streamingText="live answer"
        units={[]}
      />,
    );

    expect(screen.queryByText("No messages")).toBeNull();
    expect(container.querySelector('[data-role="reasoning"]')?.textContent).toContain(
      "private live reasoning without pub tags",
    );
    expect(container.querySelector('[data-role="assistant"]')?.textContent).toContain(
      "live answer",
    );
  });

  it("pauses auto-scroll when the user scrolls up and resumes from the jump button", () => {
    const { container } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText="live answer"
        units={[baseUnit]}
      />,
    );
    const timeline = container.querySelector(".message-timeline") as HTMLDivElement;

    Object.defineProperties(timeline, {
      clientHeight: { configurable: true, value: 200 },
      scrollHeight: { configurable: true, value: 1000 },
      scrollTop: { configurable: true, value: 600, writable: true },
    });
    fireEvent.scroll(timeline);
    fireEvent.wheel(timeline, { deltaY: -80 });

    const button = screen.getByRole("button", { name: "Jump to bottom" });
    expect(button).toBeTruthy();
    expect(button.textContent).toBe("↓");

    fireEvent.click(button);

    expect(screen.queryByRole("button", { name: "Jump to bottom" })).toBeNull();
  });

  it("resumes auto-scroll when the user manually scrolls back to the bottom", () => {
    const { container } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText="live answer"
        units={[baseUnit]}
      />,
    );
    const timeline = container.querySelector(".message-timeline") as HTMLDivElement;

    Object.defineProperties(timeline, {
      clientHeight: { configurable: true, value: 200 },
      scrollHeight: { configurable: true, value: 1000 },
      scrollTop: { configurable: true, value: 600, writable: true },
    });
    fireEvent.scroll(timeline);
    expect(screen.getByRole("button", { name: "Jump to bottom" })).toBeTruthy();

    timeline.scrollTop = 701;
    fireEvent.scroll(timeline);

    expect(screen.queryByRole("button", { name: "Jump to bottom" })).toBeNull();
  });

  it("shows the jump button when far from bottom even outside streaming", () => {
    const { container } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[baseUnit]}
      />,
    );
    const timeline = container.querySelector(".message-timeline") as HTMLDivElement;

    Object.defineProperties(timeline, {
      clientHeight: { configurable: true, value: 200 },
      scrollHeight: { configurable: true, value: 1000 },
      scrollTop: { configurable: true, value: 500, writable: true },
    });
    fireEvent.scroll(timeline);

    expect(screen.getByRole("button", { name: "Jump to bottom" })).toBeTruthy();
  });

  it("follows an appended user message using the pre-update bottom position", async () => {
    const { container, rerender } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[baseUnit]}
      />,
    );
    await nextAnimationFrame();

    const timeline = container.querySelector(".message-timeline") as HTMLDivElement;
    Object.defineProperties(timeline, {
      clientHeight: { configurable: true, value: 200 },
      scrollHeight: { configurable: true, value: 1000 },
      scrollTop: { configurable: true, value: 800, writable: true },
    });
    fireEvent.scroll(timeline);

    Object.defineProperty(timeline, "scrollHeight", {
      configurable: true,
      value: 1200,
    });
    const bottom = container.querySelector(
      ".message-timeline-content > div:last-child",
    ) as HTMLDivElement;
    const scrollIntoView = vi.fn();
    bottom.scrollIntoView = scrollIntoView;

    rerender(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[baseUnit, userUnit]}
      />,
    );

    await waitFor(() =>
      expect(scrollIntoView).toHaveBeenCalledWith({ behavior: "auto" }),
    );
  });

  it("does not follow an appended user message when already reading history", async () => {
    const { container, rerender } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[baseUnit]}
      />,
    );
    await nextAnimationFrame();

    const timeline = container.querySelector(".message-timeline") as HTMLDivElement;
    Object.defineProperties(timeline, {
      clientHeight: { configurable: true, value: 200 },
      scrollHeight: { configurable: true, value: 1000 },
      scrollTop: { configurable: true, value: 500, writable: true },
    });
    fireEvent.scroll(timeline);
    const bottom = container.querySelector(
      ".message-timeline-content > div:last-child",
    ) as HTMLDivElement;
    const scrollIntoView = vi.fn();
    bottom.scrollIntoView = scrollIntoView;

    rerender(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[baseUnit, userUnit]}
      />,
    );
    await nextAnimationFrame();

    expect(scrollIntoView).not.toHaveBeenCalled();
  });

  it("performs one final bottom sync when followed streaming content is committed", async () => {
    const { container, rerender } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText="live answer"
        units={[userUnit]}
      />,
    );
    await nextAnimationFrame();

    const bottom = container.querySelector(
      ".message-timeline-content > div:last-child",
    ) as HTMLDivElement;
    const scrollIntoView = vi.fn();
    bottom.scrollIntoView = scrollIntoView;

    rerender(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[userUnit, baseUnit]}
      />,
    );

    await waitFor(() =>
      expect(scrollIntoView).toHaveBeenCalledWith({ behavior: "auto" }),
    );
  });

  it("does not treat clearing a cancelled stream as committed output", async () => {
    const { container, rerender } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText="partial answer"
        units={[userUnit]}
      />,
    );
    await nextAnimationFrame();

    const bottom = container.querySelector(
      ".message-timeline-content > div:last-child",
    ) as HTMLDivElement;
    const scrollIntoView = vi.fn();
    bottom.scrollIntoView = scrollIntoView;

    rerender(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[userUnit]}
      />,
    );
    await nextAnimationFrame();

    expect(scrollIntoView).not.toHaveBeenCalled();
  });

  it("uses a right-side scrollbar with anchors only for user and assistant text", async () => {
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(200);
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(1000);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        const topByAnchor: Record<string, number> = {
          "unit-user": 80,
          "unit-1": 520,
        };
        const top = this.dataset.historyAnchor
          ? topByAnchor[this.dataset.historyAnchor] ?? 0
          : 0;
        return {
          bottom: top + 40,
          height: 40,
          left: 0,
          right: 30,
          top,
          width: 30,
          x: 0,
          y: top,
          toJSON: () => ({}),
        };
      },
    );

    const toolCallUnit: Unit = {
      ...baseUnit,
      uuid: "unit-tool-call",
      content: {
        role: "assistant",
        content: [
          {
            ToolCall: {
              call_id: "call-1",
              fn_name: "inspect",
              fn_arguments: {},
            },
          },
        ],
      },
    };
    const toolResultUnit: Unit = {
      ...baseUnit,
      uuid: "unit-tool-result",
      content: {
        role: "tool",
        content: [{ ToolResponse: { call_id: "call-1", content: "done" } }],
      },
    };

    const { container } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[userUnit, toolCallUnit, toolResultUnit, baseUnit]}
      />,
    );

    const rail = await screen.findByRole("scrollbar", {
      name: "Conversation position",
    });
    const userMarker = screen.getByRole("button", {
      name: "Jump to user message: Where are we?",
    });
    const assistantMarker = screen.getByRole("button", {
      name: "Jump to assistant message: final answer",
    });
    expect(userMarker.classList.contains("conversation-marker-user")).toBe(true);
    expect(
      assistantMarker.classList.contains("conversation-marker-assistant"),
    ).toBe(true);
    expect(userMarker.getAttribute("aria-current")).toBe("location");
    expect(assistantMarker.getAttribute("aria-current")).toBeNull();
    expect(
      screen.queryByRole("button", { name: /inspect|tool result/i }),
    ).toBeNull();

    const timeline = container.querySelector(".message-timeline") as HTMLDivElement;
    const scrollTo = vi.fn();
    timeline.scrollTo = scrollTo;
    fireEvent.click(assistantMarker);
    expect(scrollTo).toHaveBeenCalledWith({ behavior: "smooth", top: 508 });

    fireEvent.keyDown(rail, { key: "PageDown" });
    expect(scrollTo).toHaveBeenLastCalledWith({ behavior: "auto", top: 160 });

    fireEvent.pointerDown(rail, { clientY: 20, pointerId: 1 });
    fireEvent.pointerUp(rail, { clientY: 20, pointerId: 1 });
    expect(scrollTo).toHaveBeenLastCalledWith({ behavior: "auto", top: 400 });
    await waitFor(() => expect(rail.getAttribute("aria-valuenow")).not.toBeNull());
  });
});
