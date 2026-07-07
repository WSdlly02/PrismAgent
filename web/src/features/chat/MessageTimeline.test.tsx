import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Unit } from "../../api/types";
import { MessageTimeline } from "./MessageTimeline";

const baseUnit: Unit = {
  uuid: "unit-1",
  visibility: "public",
  content: {
    role: "assistant",
    content: [
      { ReasoningContent: "thinking through the next step" },
      { Text: "final answer" },
    ],
  },
  token_usage: null,
  metadata: {},
  created_at: 1,
};

describe("MessageTimeline", () => {
  it("renders snapshot reasoning content as its own reasoning bubble", () => {
    const { container } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[baseUnit]}
      />,
    );

    const reasoning = container.querySelector('[data-role="reasoning"]');
    const assistant = container.querySelector('[data-role="assistant"]');

    expect(reasoning?.textContent).toContain("thinking through the next step");
    expect(assistant?.textContent).toContain("final answer");
  });

  it("renders streaming reasoning separately from streaming answer text", () => {
    const { container } = render(
      <MessageTimeline
        streamingReasoningText="live reasoning"
        streamingText="live answer"
        units={[]}
      />,
    );

    expect(screen.queryByText("No messages")).toBeNull();
    expect(container.querySelector('[data-role="reasoning"]')?.textContent).toContain(
      "live reasoning",
    );
    expect(container.querySelector('[data-role="assistant"]')?.textContent).toContain(
      "live answer",
    );
  });
});
