import { CalendarDays, MessagesSquare } from "lucide-react";
import { Button } from "../ui";
import { SettingsPage } from "./SettingsPage";
import { AppList, type AppEntry } from "./apps";
import { useFlowStore } from "../state";
import { APP_NAME } from "@/flow/brand";

const calendarIcon = <CalendarDays size={16} strokeWidth={1.75} />;

const APPS: AppEntry[] = [
  {
    id: "google-calendar",
    name: "Google Calendar",
    icon: calendarIcon,
    description: "Reminders for Google meetings before they start.",
    action: "Connect",
  },
  {
    id: "outlook",
    name: "Outlook Calendar",
    icon: calendarIcon,
    description: "Reminders for Outlook meetings before they start.",
    action: "Connect",
  },
  {
    id: "slack",
    name: "Slack",
    icon: <MessagesSquare size={16} strokeWidth={1.75} />,
    description: "Adds context from your messages to meeting notes.",
    action: "Connect",
    note: "How Slack context is used",
  },
];

export default function Connectors() {
  const openSettings = useFlowStore((s) => s.openSettings);
  return (
    <SettingsPage
      title="Connections"
      intro={`Connect your apps so ${APP_NAME} can remind you about meetings and add context to notes.`}
    >
      <AppList storageKey="connectors.connected" apps={APPS} />
      <div className="flex items-center justify-between gap-[16px] border-t border-[var(--fk-border)] pt-[16px]">
        <span className="text-[14.5px] leading-[21px] text-[var(--fk-text-muted)]">
          Let your AI apps read your meeting notes.
        </span>
        <Button variant="ghost" size="sm" onClick={() => openSettings("mcp")}>
          Open AI tools
        </Button>
      </div>
    </SettingsPage>
  );
}
