import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentSummary } from "../../api/types";
import { ChatPane } from "./ChatPane";

const timelineRender = vi.hoisted(() => vi.fn());

vi.mock("./MessageTimeline", () => ({
  MessageTimeline: (props: unknown) => {
    timelineRender(props);
    return <div data-testid="message-timeline" />;
  },
}));

const agent: AgentSummary = {
  agent_uuid: "agent-1",
  agent_name: "Planner",
  profile: "planner",
  auto_loop: false,
  context_refs: [],
  context_out: [],
  status: "idle",
};

describe("ChatPane", () => {
  beforeEach(() => {
    timelineRender.mockClear();
  });

  it("keeps composer typing isolated from the message timeline", () => {
    render(
      <ChatPane
        agent={agent}
        connectionStatus="connected"
        error={null}
        pendingApproval={null}
        statusLabel="idle"
        streamingReasoningText=""
        streamingText=""
        units={[]}
        onApprove={vi.fn()}
        onCancel={vi.fn()}
        onSend={vi.fn()}
      />,
    );
    const renderCount = timelineRender.mock.calls.length;

    fireEvent.change(screen.getByPlaceholderText("Send a message"), {
      target: { value: "draft text" },
    });

    expect(screen.getByDisplayValue("draft text")).toBeTruthy();
    expect(timelineRender).toHaveBeenCalledTimes(renderCount);
  });
});
