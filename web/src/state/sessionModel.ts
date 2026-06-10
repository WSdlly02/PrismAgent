import type {
  AgentEvent,
  AgentStatus,
  PendingApproval,
  Unit,
} from "../api/types";

export type ChatState = {
  units: Unit[];
  streamingText: string;
  pendingApproval: PendingApproval | null;
  pendingUserUuid: string | null;
  status: AgentStatus | null;
  errors: string[];
};

export function initialChatState(): ChatState {
  return {
    units: [],
    streamingText: "",
    pendingApproval: null,
    pendingUserUuid: null,
    status: null,
    errors: [],
  };
}

export function applyAgentEvent(
  state: ChatState,
  event: AgentEvent,
): ChatState {
  switch (event.type) {
    case "stream_delta":
      return { ...state, streamingText: state.streamingText + event.text };
    case "unit_append": {
      const incomingRole = event.unit.content.role.toLowerCase();
      const units = incomingRole === "user" && state.pendingUserUuid
        ? state.units.filter((u) => u.uuid !== state.pendingUserUuid)
        : state.units;
      return {
        ...state,
        units: [...units, event.unit],
        pendingUserUuid: incomingRole === "user" ? null : state.pendingUserUuid,
        streamingText: "",
      };
    }
    case "approve_request":
      return { ...state, pendingApproval: event.request };
    case "status_changed":
      return { ...state, status: event.status };
    case "error":
      return { ...state, errors: [...state.errors, event.message] };
  }
}
