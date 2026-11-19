import type { AgentAddr, AgentEvent, WorkspaceEvent, WsClientMessage, WsServerMessage } from "./types";

export type { WsClientMessage, WsServerMessage };

export function createWebSocket(): WebSocket {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return new WebSocket(`${protocol}//${window.location.host}/ws`);
}

export function wsSend(ws: WebSocket, msg: WsClientMessage): void {
  if (ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify(msg));
  }
}

export function parseWsMessage(data: string): WsServerMessage {
  return JSON.parse(data) as WsServerMessage;
}

export type SubscriptionEvent = AgentEvent | WorkspaceEvent;

type Observer = {
  load: () => Promise<() => void>;
  receive: (event: SubscriptionEvent) => void;
  disconnected: () => void;
  failed: (error: unknown) => void;
};

// A socket never changes its target. Control replies and live events use
// separate server queues, so an event can even precede the acknowledgement.
export function watchSubscription(target: string | AgentAddr, observer: Observer): () => void {
  const workspaceOnly = typeof target === "string";
  let socket: WebSocket | null = null;
  let retry: ReturnType<typeof setTimeout> | undefined;
  function connect() {
    const ws = createWebSocket();
    socket = ws;
    let pending: SubscriptionEvent[] | null = [];
    let failed = false;
    const fail = (error: unknown) => {
      failed = true;
      pending = null;
      observer.failed(error);
    };
    ws.onopen = () => {
      if (socket !== ws) return;
      wsSend(ws, { type: "subscribe_workspace", workspace_uuid: workspaceOnly ? target : target.workspace_uuid });
    };
    ws.onmessage = async ({ data }) => {
      if (socket !== ws) return;
      const msg = parseWsMessage(data as string);
      if (msg.type === "ping") { wsSend(ws, { type: "pong" }); return; }
      if (failed || msg.type === "connected") return;
      if (msg.type === "error") { fail(new Error(msg.error.message)); return; }
      if (msg.type === "workspace_subscribed" && !workspaceOnly) {
        wsSend(ws, { type: "subscribe_agent", agent_uuid: target.agent_uuid });
        return;
      }
      if (msg.type === "workspace_subscribed" || msg.type === "agent_subscribed") {
        try {
          const apply = await observer.load();
          if (socket !== ws || failed) return;
          apply();
          for (const event of pending ?? []) observer.receive(event);
          pending = null;
        } catch (error) {
          if (socket === ws) fail(error);
        }
        return;
      }
      if (pending) pending.push(msg);
      else observer.receive(msg);
    };
    ws.onclose = () => {
      if (socket !== ws) return;
      socket = null;
      observer.disconnected();
      retry = setTimeout(connect, 1000);
    };
  }
  connect();
  return () => {
    clearTimeout(retry);
    const previous = socket;
    socket = null;
    previous?.close();
  };
}
