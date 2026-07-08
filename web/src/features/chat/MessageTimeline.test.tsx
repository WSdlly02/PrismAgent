import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
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
});
