import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, acquireLease, createAgent, releaseLease } from "../api/client";
import { usePrismSession } from "./usePrismSession";

vi.mock("../api/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api/client")>();
  return {
    ...actual,
    acquireLease: vi.fn(),
    createAgent: vi.fn(),
    releaseLease: vi.fn(),
  };
});

const AGENT_INPUT = {
  name: "Planner",
  profile: "planner",
  context_refs: [],
  context_out: [],
};

describe("usePrismSession workspace leases", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(acquireLease).mockReset();
    vi.mocked(createAgent).mockReset();
    vi.mocked(releaseLease).mockReset();
    vi.mocked(createAgent).mockResolvedValue({ created: true });
    vi.mocked(releaseLease).mockResolvedValue({ released: true });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("does not renew an inactive workspace lease on a timer", async () => {
    vi.mocked(acquireLease).mockResolvedValue({
      workspace_uuid: "workspace-1",
      client_id: "client-1",
      lease_token: "lease-1",
      expires_at: Math.floor(Date.now() / 1000) + 10,
    });
    const { result } = renderHook(() => usePrismSession());

    await act(async () => {
      await result.current.createAgent("workspace-1", AGENT_INPUT);
    });
    await act(async () => {
      vi.advanceTimersByTime(30_000);
    });

    expect(acquireLease).toHaveBeenCalledTimes(1);
  });

  it("exposes a user-facing error when another client holds the lease", async () => {
    vi.mocked(acquireLease).mockRejectedValue(
      new ApiError("conflict: workspace_lease workspace-1", 409),
    );
    const { result } = renderHook(() => usePrismSession());

    await act(async () => {
      await result.current.createAgent("workspace-1", AGENT_INPUT).catch(() => {});
    });

    expect(result.current.error).toBe(
      "This workspace is currently in use by another client. Try again shortly.",
    );
  });
});
