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
});
