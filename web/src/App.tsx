import { useEffect, useRef, useState } from "react";
import { AppShell } from "./components/AppShell";
import { ChatPane } from "./features/chat/ChatPane";
import { InspectorPane } from "./features/inspector/InspectorPane";
import { WorkspaceSidebar } from "./features/workspaces/WorkspaceSidebar";
import { usePrismSession } from "./state/usePrismSession";

export function App() {
  const session = usePrismSession();
  const didLoadInitialData = useRef(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);

  useEffect(() => {
    if (didLoadInitialData.current) {
      return;
    }
    didLoadInitialData.current = true;
    void session.loadInitialData();
  }, [session.loadInitialData]);

  return (
    <AppShell
      sidebarCollapsed={sidebarCollapsed}
      sidebar={
        <WorkspaceSidebar
          workspaces={session.workspaces}
          profiles={session.profiles}
          expandedWorkspaceUuids={session.expandedWorkspaceUuids}
          workspaceAgents={session.workspaceAgents}
          workspaceErrors={session.workspaceErrors}
          selectedAddr={session.selectedAddr}
          onSelectWorkspace={session.expandWorkspace}
          onSelectAgent={session.selectAgent}
          onAddWorkspace={session.addWorkspace}
          onCreateAgent={session.createAgent}
          onDeleteAgent={session.deleteAgent}
          onDeleteWorkspace={session.deleteWorkspace}
          collapsed={sidebarCollapsed}
          onToggleCollapse={() => setSidebarCollapsed((v) => !v)}
        />
      }
      chat={
        <ChatPane
          key={session.selectedAddr ? `${session.selectedAddr.workspace_uuid}/${session.selectedAddr.agent_uuid}` : "no-agent"}
          agent={session.selectedAgent}
          connectionStatus={session.connectionStatus}
          error={session.error}
          pendingApproval={session.pendingApproval}
          statusLabel={session.statusLabel}
          streamingReasoningText={session.streamingReasoningText}
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
          session={session.selectedAddr}
          workspace={session.selectedWorkspace}
        />
      }
    />
  );
}
