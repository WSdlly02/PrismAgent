import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { marked } from "marked";
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

const toolArgumentLines = [
  "printf 'this argument is intentionally longer than sixty characters and must remain complete'",
  "echo 'the second command line must render on its own line'",
];
const fullToolArgument = toolArgumentLines.join("\n");
const plainToolResult =
  "tool output: this result is intentionally longer than one hundred and twenty characters so the expanded bubble must preserve every character returned by the tool without applying a preview limit";
const fullToolResult = {
  command: fullToolArgument,
  output: "first line\nsecond line\nthird line",
  status: "success",
};

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
          fn_arguments: {
            command: fullToolArgument,
            options: { include_hidden: true },
          },
        },
      },
      {
        ReasoningContent: "reasoning before the tool call",
      },
    ],
  },
};

const toolResultUnit: Unit = {
  ...baseUnit,
  uuid: "unit-tool-result",
  content: {
    role: "tool",
    content: [
      {
        ToolResponse: {
          call_id: "call-1",
          fn_name: "inspect",
          content: JSON.stringify(fullToolResult),
        },
      },
    ],
  },
};

function nextAnimationFrame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()));
}

describe("MessageTimeline", () => {
  it("renders snapshot reasoning as a separate collapsed bubble", () => {
    const { container } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[baseUnit]}
      />,
    );

    const reasoning = container.querySelector(
      'details[data-role="reasoning"]',
    ) as HTMLDetailsElement;
    const assistant = container.querySelector('[data-role="assistant"]');

    expect(reasoning.open).toBe(false);
    expect(reasoning.textContent).toContain(
      "historical reasoning should not be rendered",
    );
    expect(assistant?.textContent).toContain("final answer");
    expect(
      Array.from(container.querySelectorAll(".message")).map(
        (message) => message.getAttribute("data-role"),
      ),
    ).toEqual(["reasoning", "assistant"]);
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

  it("wraps Markdown tables in a keyboard-accessible scroll area", () => {
    const tableUnit: Unit = {
      ...baseUnit,
      uuid: "unit-table",
      content: {
        role: "assistant",
        content: [
          {
            Text: [
              "| Name | Details |",
              "| --- | --- |",
              "| PrismAgent | A deliberately wide table value |",
            ].join("\n"),
          },
        ],
      },
    };

    const { container } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[tableUnit]}
      />,
    );

    const scrollArea = container.querySelector(".markdown-table-scroll");
    expect(scrollArea?.getAttribute("role")).toBe("region");
    expect(scrollArea?.getAttribute("aria-label")).toBe("Scrollable table");
    expect(scrollArea?.getAttribute("tabindex")).toBe("0");
    expect(scrollArea?.querySelector("table")).toBeTruthy();
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

  it("does not reparse committed messages during streaming updates", () => {
    const parse = vi.spyOn(marked, "parse");
    const units = [baseUnit];
    const { rerender } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={units}
      />,
    );
    parse.mockClear();

    rerender(
      <MessageTimeline
        streamingReasoningText=""
        streamingText="partial answer"
        units={units}
      />,
    );
    expect(parse).toHaveBeenCalledTimes(1);

    parse.mockClear();
    rerender(
      <MessageTimeline
        streamingReasoningText="partial reasoning"
        streamingText="partial answer"
        units={units}
      />,
    );
    expect(parse).toHaveBeenCalledTimes(1);

    parse.mockClear();
    rerender(
      <MessageTimeline
        streamingReasoningText="partial reasoning"
        streamingText="partial answer"
        units={[...units, userUnit]}
      />,
    );
    expect(parse).toHaveBeenCalledTimes(1);
  });

  it("collapses tool calls and tool results independently by default", () => {
    const { container } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[toolCallUnit, toolResultUnit]}
      />,
    );

    const toolCall = container.querySelector(
      'details[data-role="tool_call"]',
    ) as HTMLDetailsElement;
    const toolResult = container.querySelector(
      'details[data-role="tool"]',
    ) as HTMLDetailsElement;

    expect(toolCall.open).toBe(false);
    expect(toolResult.open).toBe(false);
    expect(
      Array.from(container.querySelectorAll(".message")).map(
        (message) => message.getAttribute("data-role"),
      ),
    ).toEqual(["reasoning", "tool_call", "tool"]);
    expect(
      container.querySelector('details[data-role="reasoning"]')?.textContent,
    ).toContain("reasoning before the tool call");

    fireEvent.click(toolCall.querySelector("summary") as HTMLElement);

    expect(toolCall.open).toBe(true);
    expect(toolResult.open).toBe(false);
    expect(toolCall.querySelector(".tool-content")?.textContent).toBe(
      [
        "{",
        '  "command": """',
        ...toolArgumentLines.map((line) => `    ${line}`),
        '  """,',
        '  "options": {',
        '    "include_hidden": true',
        "  }",
        "}",
      ].join("\n"),
    );
    expect(toolResult.querySelector(".tool-content")?.textContent).toBe(
      [
        "{",
        '  "command": """',
        ...toolArgumentLines.map((line) => `    ${line}`),
        '  """,',
        '  "output": """',
        "    first line",
        "    second line",
        "    third line",
        '  """,',
        '  "status": "success"',
        "}",
      ].join("\n"),
    );
  });

  it("preserves non-JSON tool results verbatim", () => {
    const plainResultUnit: Unit = {
      ...toolResultUnit,
      uuid: "unit-plain-tool-result",
      content: {
        role: "tool",
        content: [
          {
            ToolResponse: {
              call_id: "call-plain",
              fn_name: "inspect",
              content: plainToolResult,
            },
          },
        ],
      },
    };

    const { container } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText=""
        units={[plainResultUnit]}
      />,
    );

    expect(container.querySelector(".tool-content")?.textContent).toBe(
      plainToolResult,
    );
  });

  it("preserves an expanded reasoning bubble across streaming updates", () => {
    const { container, rerender } = render(
      <MessageTimeline
        streamingReasoningText="first reasoning chunk"
        streamingText=""
        units={[]}
      />,
    );
    const reasoning = container.querySelector(
      'details[data-role="reasoning"]',
    ) as HTMLDetailsElement;

    expect(reasoning.open).toBe(false);
    fireEvent.click(reasoning.querySelector("summary") as HTMLElement);
    expect(reasoning.open).toBe(true);

    rerender(
      <MessageTimeline
        streamingReasoningText="first reasoning chunk and more"
        streamingText=""
        units={[]}
      />,
    );

    expect(container.querySelector('details[data-role="reasoning"]')).toBe(
      reasoning,
    );
    expect(reasoning.open).toBe(true);
    expect(reasoning.textContent).toContain("and more");
  });

  it("pauses auto-scroll when the user scrolls up and resumes from the jump button", async () => {
    const { container } = render(
      <MessageTimeline
        streamingReasoningText=""
        streamingText="live answer"
        units={[baseUnit]}
      />,
    );
    const timeline = container.querySelector(".message-timeline") as HTMLDivElement;
    const bottom = container.querySelector(
      ".message-timeline-content > div:last-child",
    ) as HTMLDivElement;
    const scrollIntoView = vi.fn();
    bottom.scrollIntoView = scrollIntoView;

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

    await nextAnimationFrame();
    expect(scrollIntoView).not.toHaveBeenCalled();

    fireEvent.click(button);

    expect(screen.queryByRole("button", { name: "Jump to bottom" })).toBeNull();
    expect(scrollIntoView).toHaveBeenCalledWith({ behavior: "smooth" });
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
