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
  status: AgentStatus | null;
  errors: string[];
};

export function initialChatState(): ChatState {
  return {
    units: [],
    streamingText: "",
    pendingApproval: null,
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
    case "unit_append":
      return {
        ...state,
        units: [...state.units, event.unit],
        streamingText: "",
      };
    case "approve_request":
      return { ...state, pendingApproval: event.request };
    case "status_changed":
      return { ...state, status: event.status };
    case "error":
      return { ...state, errors: [...state.errors, event.message] };
  }
}
