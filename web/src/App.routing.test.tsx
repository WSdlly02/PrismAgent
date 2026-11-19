import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { App } from "./App";
import * as api from "./api/client";
import type { AgentSnapshot, AgentSummary, Unit, WsClientMessage, WsServerMessage } from "./api/types";

class Socket {
  static readonly OPEN = 1;
  static instances: Socket[] = [];
  readyState = Socket.OPEN;
  sent: WsClientMessage[] = [];
  onopen?: () => void;
  onclose?: () => void;
  onmessage?: (frame: { data: string }) => void;
  constructor() {
    Socket.instances.push(this);
    queueMicrotask(() => this.onopen?.());
  }
  static autoAck = true;
  send(data: string) {
    const msg: WsClientMessage = JSON.parse(data);
    this.sent.push(msg);
    if (Socket.autoAck && msg.type === "subscribe_workspace") queueMicrotask(() => this.frame({ type: "workspace_subscribed" }));
    if (Socket.autoAck && msg.type === "subscribe_agent") queueMicrotask(() => this.frame({ type: "agent_subscribed" }));
  }
  close() { this.readyState = 3; this.onclose?.(); }
  frame(msg: WsServerMessage) { this.onmessage?.({ data: JSON.stringify(msg) }); }
}

afterEach(() => { Socket.instances = []; Socket.autoAck = true; vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("isolates identical agent IDs across sidebar actions, late frames, snapshots and reconnects", async () => {
  vi.stubGlobal("WebSocket", Socket);
  const agent: AgentSummary = {
    agent_name: "Same", agent_uuid: "same", profile: "default", auto_loop: false,
    context_refs: [], context_out: [], status: "idle",
  };
  const a = { workspace_uuid: "a", agent_uuid: "same" };
  const b = { workspace_uuid: "b", agent_uuid: "same" };
  const empty: AgentSnapshot = { units: [], status: "idle", pending_approval: null };
  let finishA!: (value: AgentSnapshot) => void;
  vi.spyOn(api, "listWorkspaces").mockResolvedValue(["a", "b"].map(id => ({
    workspace_uuid: id, workspace_path: "/" + id, locked_by: null,
  })));
  vi.spyOn(api, "listProfiles").mockResolvedValue([]);
  vi.spyOn(api, "listAgents").mockResolvedValue([agent]);
  vi.spyOn(api, "agentSnapshot").mockImplementation(addr => addr.workspace_uuid === "a"
    ? new Promise(resolve => { finishA = resolve; }) : Promise.resolve(empty));
  vi.spyOn(api, "acquireLease").mockImplementation(async (ws, client) => ({
    workspace_uuid: ws, client_id: client, lease_token: "lease-" + ws, expires_at: Date.now() / 1000 + 10,
  }));
  vi.spyOn(api, "releaseLease").mockResolvedValue({ released: true });
  vi.spyOn(api, "sendMessage").mockResolvedValue({ accepted: true });
  vi.spyOn(api, "deleteAgent").mockResolvedValue({ deleted: true });
  const view = render(<App />);
  const folder = (ws: string) => within(screen.getByTitle("/" + ws).closest(".ws-folder") as HTMLElement);
  const row = (ws: string) => folder(ws).getByText("Same").closest("button")!;
  const chatSocket = (ws: string) => Socket.instances.find(socket =>
    socket.readyState === Socket.OPEN && socket.sent[0]?.type === "subscribe_workspace" &&
    socket.sent[0].workspace_uuid === ws && socket.sent[1]?.type === "subscribe_agent");
  await screen.findByTitle("/a");
  fireEvent.doubleClick(screen.getByTitle("/a"));
  await folder("a").findByText("Same");
  fireEvent.doubleClick(screen.getByTitle("/b"));
  await folder("b").findByText("Same");
  const workspaceSocket = (ws: string) => Socket.instances.find(socket =>
    socket.readyState === Socket.OPEN && socket.sent.length === 1 &&
    socket.sent[0]?.type === "subscribe_workspace" && socket.sent[0].workspace_uuid === ws);
  act(() => workspaceSocket("a")!.frame({ type: "agent_status_changed", agent_uuid: "same", status: "running_tool" }));
  expect(within(row("a")).getByLabelText("running_tool")).toBeTruthy();
  expect(within(row("b")).getByLabelText("idle")).toBeTruthy();
  act(() => workspaceSocket("a")!.frame({ type: "agent_status_changed", agent_uuid: "same", status: "idle" }));
  const sidebarA = workspaceSocket("a")!;
  const sidebarB = workspaceSocket("b")!;
  fireEvent.click(row("a"));
  await waitFor(() => expect(chatSocket("a")).toBeDefined());
  const old = chatSocket("a")!;
  fireEvent.click(row("b"));
  await waitFor(() => expect(chatSocket("b")).toBeDefined());
  expect(workspaceSocket("a")).toBe(sidebarA);
  expect(workspaceSocket("b")).toBe(sidebarB);
  expect(api.agentSnapshot).toHaveBeenCalledWith(a);
  expect(api.agentSnapshot).toHaveBeenCalledWith(b);
  expect(row("a").closest(".ws-agent-row")?.getAttribute("data-active")).toBe("false");
  expect(row("b").closest(".ws-agent-row")?.getAttribute("data-active")).toBe("true");
  await act(async () => {
    old.frame({ type: "status_changed", status: "running_llm" });
    finishA({ ...empty, status: "running_llm" });
  });
  expect(screen.getByRole("button", { name: "Send" })).toBeTruthy();
  fireEvent.change(screen.getByPlaceholderText("Send a message"), { target: { value: "hello B" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await waitFor(() => expect(api.sendMessage).toHaveBeenCalledWith(b, "lease-b", "hello B"));

  const beforeReconnect = chatSocket("b")!;
  act(() => beforeReconnect.close());
  await waitFor(() => expect(chatSocket("b")?.sent).toEqual([
    { type: "subscribe_workspace", workspace_uuid: "b" }, { type: "subscribe_agent", agent_uuid: "same" },
  ]), { timeout: 3000 });
  const error = { code: "test", message: "A subscription failed", retryable: false };
  act(() => sidebarA.frame({ type: "error", error }));
  expect(folder("a").getByRole("alert").textContent).toBe(error.message);
  expect(view.container.querySelector(".chat-pane .error-banner")).toBeNull();
  act(() => chatSocket("b")!.frame({ type: "error", error: { ...error, message: "B chat failed" } }));
  expect(view.container.querySelector(".chat-pane .error-banner")?.textContent).toBe("B chat failed");
  fireEvent.click(folder("a").getByRole("button", { name: "Delete agent Same" }));
  fireEvent.click(screen.getByRole("button", { name: "Delete" }));
  await waitFor(() => expect(api.deleteAgent).toHaveBeenCalledWith(a, "lease-a"));
  await waitFor(() => expect(folder("a").queryByText("Same")).toBeNull());
  fireEvent.doubleClick(screen.getByTitle("/a"));
  expect(row("b").closest(".ws-agent-row")?.getAttribute("data-active")).toBe("true");
  expect(chatSocket("b")).toBeDefined();
  view.unmount();
  expect(Socket.instances.every(socket => socket.readyState !== Socket.OPEN)).toBe(true);
});

it("waits for subscription readiness and reconciles overlapping frames on initial load and reconnect", async () => {
  Socket.autoAck = false;
  vi.stubGlobal("WebSocket", Socket);
  const addr = { workspace_uuid: "ws", agent_uuid: "same" };
  const agent: AgentSummary = {
    ...addr, agent_name: "Example", profile: "default", auto_loop: false,
    context_refs: [], context_out: [], status: "idle",
  };
  vi.spyOn(api, "listWorkspaces").mockResolvedValue([{ workspace_uuid: "ws", workspace_path: "/ws", locked_by: null }]);
  vi.spyOn(api, "listProfiles").mockResolvedValue([]);
  vi.spyOn(api, "listAgents").mockResolvedValue([agent]);
  let resolve!: (snapshot: AgentSnapshot) => void;
  const fetchSnapshot = vi.spyOn(api, "agentSnapshot").mockImplementation(() => new Promise(r => { resolve = r; }));
  const view = render(<App />);
  const unit = (uuid: string): Unit => ({
    uuid, visibility: "public", content: { role: "user", content: uuid }, metadata: {}, token_usage: null, created_at: 1,
  });
  const snapshot = (...ids: string[]): AgentSnapshot => ({
    units: ids.map(unit), status: "idle", pending_approval: null,
  });
  fireEvent.doubleClick(await screen.findByTitle("/ws"));
  await act(async () => Socket.instances[0].frame({ type: "workspace_subscribed" }));
  fireEvent.click(await screen.findByText("Example"));
  const pane = within(view.container.querySelector<HTMLElement>(".chat-pane")!);
  const chat = Socket.instances[1];
  act(() => chat.frame({ type: "connected" }));
  act(() => chat.frame({ type: "workspace_subscribed" }));
  expect(fetchSnapshot).not.toHaveBeenCalled();
  act(() => chat.frame({ type: "unit_append", unit: unit("before-ack") }));
  act(() => chat.frame({ type: "agent_subscribed" }));
  expect(fetchSnapshot).toHaveBeenCalledWith(addr);
  act(() => {
    chat.frame({ type: "unit_append", unit: unit("overlap") });
    chat.frame({ type: "unit_append", unit: unit("live-only") });
    chat.frame({ type: "status_changed", status: "running_llm" });
  });
  await act(async () => resolve(snapshot("history", "before-ack", "overlap")));
  for (const id of ["history", "before-ack", "overlap", "live-only"]) expect(pane.getAllByText(id)).toHaveLength(1);
  expect(pane.getByRole("button", { name: "Cancel" })).toBeTruthy();

  act(() => chat.close());
  await waitFor(() => expect(Socket.instances).toHaveLength(3), { timeout: 3000 });
  const resumed = Socket.instances[2];
  act(() => {
    resumed.frame({ type: "workspace_subscribed" });
    resumed.frame({ type: "agent_subscribed" });
    chat.frame({ type: "unit_append", unit: unit("obsolete") });
    resumed.frame({ type: "unit_append", unit: unit("after-reconnect") });
  });
  expect(fetchSnapshot).toHaveBeenCalledTimes(2);
  await act(async () => resolve(snapshot("history", "before-ack", "overlap", "live-only", "while-offline")));
  act(() => resumed.frame({ type: "unit_append", unit: unit("while-offline") }));
  expect(pane.getAllByText("while-offline")).toHaveLength(1);
  expect(pane.getByText("after-reconnect")).toBeTruthy();
  expect(pane.queryByText("obsolete")).toBeNull();
  expect(pane.getByRole("button", { name: "Send" })).toBeTruthy();
  view.unmount();
});
