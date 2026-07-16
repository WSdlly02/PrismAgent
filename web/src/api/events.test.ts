import { describe, expect, it } from "vitest";
import { createWebSocket, parseWsMessage, wsSend } from "./events";
import type { WsClientMessage } from "./types";

describe("websocket helpers", () => {
  it("creates a WebSocket with the correct protocol and path", () => {
    const originalLocation = window.location;
    Object.defineProperty(window, "location", {
      configurable: true,
      value: { protocol: "https:", host: "example.com" },
    });

    const ws = createWebSocket();
    expect(ws.url).toBe("wss://example.com/ws");
    ws.close();

    Object.defineProperty(window, "location", {
      configurable: true,
      value: { protocol: "http:", host: "localhost:3000" },
    });

    const ws2 = createWebSocket();
    expect(ws2.url).toBe("ws://localhost:3000/ws");
    ws2.close();

    Object.defineProperty(window, "location", {
      configurable: true,
      value: originalLocation,
    });
  });

  it("serializes client messages to JSON and sends when open", () => {
    const sent: string[] = [];
    const ws = {
      readyState: 1, // WebSocket.OPEN
      send(data: string) {
        sent.push(data);
      },
    } as unknown as WebSocket;

    const msg: WsClientMessage = {
      type: "subscribe_workspace",
      workspace_uuid: "ws-1",
    };
    wsSend(ws, msg);
    expect(JSON.parse(sent[0])).toEqual(msg);
  });

  it("does not send when WebSocket is not open", () => {
    const sent: string[] = [];
    const ws = {
      readyState: 0, // WebSocket.CONNECTING
      send(data: string) {
        sent.push(data);
      },
    } as unknown as WebSocket;

    wsSend(ws, { type: "pong" });
    expect(sent).toHaveLength(0);
  });

  it("parses server JSON messages into WsServerMessage", () => {
    const connected = parseWsMessage('{"type":"connected"}');
    expect(connected).toEqual({ type: "connected" });

    const ping = parseWsMessage('{"type":"ping","ts":123}');
    expect(ping).toEqual({ type: "ping", ts: 123 });

    const error = parseWsMessage(
      '{"type":"error","error":{"code":"validation_failed","message":"invalid message","retryable":false}}',
    );
    expect(error).toEqual({
      type: "error",
      error: {
        code: "validation_failed",
        message: "invalid message",
        retryable: false,
      },
    });

    const agentEvent = parseWsMessage(
      JSON.stringify({
        type: "unit_append",
        unit: {
          uuid: "u1",
          visibility: "public",
          content: { role: "assistant", content: "hi" },
          token_usage: null,
          metadata: {},
          created_at: 1,
        },
      }),
    );
    expect(agentEvent).toEqual({
      type: "unit_append",
      unit: {
        uuid: "u1",
        visibility: "public",
        content: { role: "assistant", content: "hi" },
        token_usage: null,
        metadata: {},
        created_at: 1,
      },
    });
  });
});
