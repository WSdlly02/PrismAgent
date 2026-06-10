import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { usePrismSession, type PrismSession } from "./state/usePrismSession";

vi.mock("./state/usePrismSession", () => ({
  usePrismSession: vi.fn(),
}));

function session(loadFn: () => Promise<void>): PrismSession {
  return {
    clientId: "client-1",
    workspaces: [],
    profiles: [],
    expandedWorkspaceUuids: [],
    workspaceSessions: {},
    workspaceAgents: {},
    selectedAgent: null,
    selectedWorkspace: null,
    session: null,
    units: [],
    streamingText: "",
    pendingApproval: null,
    statusLabel: "idle",
    connectionStatus: "idle",
    error: null,
    loadInitialData: loadFn,
    expandWorkspace: vi.fn(),
    selectAgent: vi.fn(),
    addWorkspace: vi.fn(),
    createAgent: vi.fn(),
    send: vi.fn(),
    cancel: vi.fn(),
    approve: vi.fn(),
  };
}

describe("App initialization", () => {
  beforeEach(() => {
    vi.mocked(usePrismSession).mockReset();
  });

  it("loads initial data once even when session callbacks change identity", async () => {
    const firstLoad = vi.fn(async () => undefined);
    const secondLoad = vi.fn(async () => undefined);
    vi.mocked(usePrismSession)
      .mockReturnValueOnce(session(firstLoad))
      .mockReturnValueOnce(session(secondLoad));

    const { rerender } = render(<App />);
    rerender(<App />);

    await waitFor(() => expect(firstLoad).toHaveBeenCalledTimes(1));
    expect(secondLoad).not.toHaveBeenCalled();
  });
});
