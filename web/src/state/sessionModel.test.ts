import { describe, expect, it } from "vitest";
import { applyAgentEvent, initialChatState } from "./sessionModel";
import type { Unit } from "../api/types";

const unit: Unit = {
  uuid: "unit-1",
  visibility: "public",
  content: { role: "assistant", content: [{ Text: "done" }] },
  token_usage: null,
  metadata: {},
  created_at: 1,
};

describe("session model", () => {
  it("accumulates stream deltas in a draft", () => {
    const next = applyAgentEvent(initialChatState(), {
      type: "stream_delta",
      text: "hel",
    });

    expect(
      applyAgentEvent(next, { type: "stream_delta", text: "lo" }).streamingText,
    ).toBe("hello");
  });

  it("appends committed units and clears streaming drafts", () => {
    const next = applyAgentEvent(
      { ...initialChatState(), streamingText: "draft" },
      { type: "unit_append", unit },
    );

    expect(next.units).toEqual([unit]);
    expect(next.streamingText).toBe("");
  });

  it("replaces only the active pending user unit when the backend user unit arrives", () => {
    const pending: Unit = {
      ...unit,
      uuid: "__pending-1",
      content: { role: "user", content: "hello" },
    };
    const otherPending: Unit = {
      ...pending,
      uuid: "__pending-2",
    };
    const backendUser: Unit = {
      ...pending,
      uuid: "unit-user-1",
    };

    const next = applyAgentEvent(
      { ...initialChatState(), units: [pending, otherPending], pendingUserUuid: pending.uuid },
      { type: "unit_append", unit: backendUser },
    );

    expect(next.units).toEqual([otherPending, backendUser]);
    expect(next.pendingUserUuid).toBeNull();
  });

  it("keeps pending user units when non-user units arrive", () => {
    const pending: Unit = {
      ...unit,
      uuid: "__pending-1",
      content: { role: "user", content: "hello" },
    };

    const next = applyAgentEvent(
      { ...initialChatState(), units: [pending], pendingUserUuid: pending.uuid },
      { type: "unit_append", unit },
    );

    expect(next.units).toEqual([pending, unit]);
    expect(next.pendingUserUuid).toBe(pending.uuid);
  });

  it("stores pending approval requests", () => {
    const next = applyAgentEvent(initialChatState(), {
      type: "approve_request",
      request: {
        request_uuid: "approval-1",
        description: "Run tool?",
        tool_count: 3,
        auto_approved_mask: 0b010,
        manual_approval_mask: 0b101,
      },
    });

    expect(next.pendingApproval?.request_uuid).toBe("approval-1");
  });

  it("updates status and records errors", () => {
    const withStatus = applyAgentEvent(initialChatState(), {
      type: "status_changed",
      status: "running_llm",
    });
    const withError = applyAgentEvent(withStatus, {
      type: "error",
      message: "boom",
    });

    expect(withError.status).toBe("running_llm");
    expect(withError.errors).toEqual(["boom"]);
  });
});
