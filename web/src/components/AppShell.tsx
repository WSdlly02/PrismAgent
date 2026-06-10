import type { ReactNode } from "react";

type AppShellProps = {
  sidebar: ReactNode;
  chat: ReactNode;
  inspector: ReactNode;
  sidebarCollapsed?: boolean;
};

export function AppShell({ sidebar, chat, inspector, sidebarCollapsed }: AppShellProps) {
  return (
    <div className={`app-shell${sidebarCollapsed ? " sidebar-collapsed-shell" : ""}`}>
      <aside className={`app-column app-column-left${sidebarCollapsed ? " sidebar-collapsed" : ""}`}>
        {sidebar}
      </aside>
      <section className="app-column app-column-center">{chat}</section>
      <aside className="app-column app-column-right">{inspector}</aside>
    </div>
  );
}
