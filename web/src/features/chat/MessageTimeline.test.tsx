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
        ReasoningContent:
          "private <pub>thinking through the next step</pub> hidden",
      },
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
    expect(reasoning?.textContent).not.toContain("private");
    expect(reasoning?.textContent).not.toContain("hidden");
  });

  it("renders streaming reasoning separately from streaming answer text", () => {
    const { container } = render(
      <MessageTimeline
        streamingReasoningText="private <pub>live reasoning</pub>"
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

  it("renders multiple public reasoning blocks as separate bubbles", () => {
    const unit: Unit = {
      ...baseUnit,
      content: {
        role: "assistant",
        content: [
          {
            ReasoningContent:
              "<pub>first public thought</pub> private <pub>second public thought</pub>",
          },
          { Text: "done" },
        ],
      },
    };

    const { container } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[unit]}
      />,
    );

    const reasoning = [...container.querySelectorAll('[data-role="reasoning"]')];
    expect(reasoning).toHaveLength(2);
    expect(reasoning[0].textContent).toContain("first public thought");
    expect(reasoning[1].textContent).toContain("second public thought");
  });

  it("ignores incomplete and unmatched public reasoning tags", () => {
    const unit: Unit = {
      ...baseUnit,
      content: {
        role: "assistant",
        content: [
          {
            ReasoningContent:
              "private <pub>unfinished public thought </pub> orphan </pub> <pub>not closed",
          },
          { Text: "done" },
        ],
      },
    };

    const { container } = render(
      <MessageTimeline
        streamingReasoningText="stream private <pub>not closed yet"
        streamingText=""
        units={[unit]}
      />,
    );

    const reasoning = [...container.querySelectorAll('[data-role="reasoning"]')];
    expect(reasoning).toHaveLength(1);
    expect(reasoning[0].textContent).toContain("unfinished public thought");
    expect(reasoning[0].textContent).not.toContain("not closed");
  });
});
