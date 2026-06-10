import type { ReactNode } from "react";

type AppShellProps = {
  sidebar: ReactNode;
  chat: ReactNode;
  inspector: ReactNode;
};

export function AppShell({ sidebar, chat, inspector }: AppShellProps) {
  return (
    <div className="app-shell">
      <aside className="app-column app-column-left">{sidebar}</aside>
      <section className="app-column app-column-center">{chat}</section>
      <aside className="app-column app-column-right">{inspector}</aside>
    </div>
  );
}