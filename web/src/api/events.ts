import type { WsClientMessage, WsServerMessage } from "./types";

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
