import { useEffect, useRef } from "react";
import { AppShell } from "./components/AppShell";
import { ChatPane } from "./features/chat/ChatPane";
import { InspectorPane } from "./features/inspector/InspectorPane";
import { WorkspaceSidebar } from "./features/workspaces/WorkspaceSidebar";
import { usePrismSession } from "./state/usePrismSession";

export function App() {
  const session = usePrismSession();
  const didLoadInitialData = useRef(false);

  useEffect(() => {
    if (didLoadInitialData.current) {
      return;
    }
    didLoadInitialData.current = true;
    void session.loadInitialData();
  }, [session.loadInitialData]);

  return (
    <AppShell
      sidebar={
        <WorkspaceSidebar
          workspaces={session.workspaces}
          profiles={session.profiles}
          expandedWorkspaceUuids={session.expandedWorkspaceUuids}
          workspaceAgents={session.workspaceAgents}
          selectedAgentUuid={session.selectedAgent?.agent_uuid ?? null}
          onSelectWorkspace={session.expandWorkspace}
          onSelectAgent={session.selectAgent}
          onAddWorkspace={session.addWorkspace}
          onCreateAgent={session.createAgent}
        />
      }
      chat={
        <ChatPane
          agent={session.selectedAgent}
          connectionStatus={session.connectionStatus}
          error={session.error}
          pendingApproval={session.pendingApproval}
          statusLabel={session.statusLabel}
          streamingText={session.streamingText}
          units={session.units}
          onApprove={session.approve}
          onCancel={session.cancel}
          onSend={session.send}
        />
      }
      inspector={
        <InspectorPane
          agent={session.selectedAgent}
          session={session.session}
          workspace={session.selectedWorkspace}
        />
      }
    />
  );
}
