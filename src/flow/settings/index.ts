import type { ComponentType } from "react";
import type { SettingsTab } from "../state";
import General from "./General";
import System from "./System";
import NotetakerSettings from "./NotetakerSettings";
import VibeCoding from "./VibeCoding";
import Connectors from "./Connectors";
import Mcp from "./Mcp";
import Account from "./Account";
import About from "./About";
import Privacy from "./Privacy";

export const SETTINGS_PAGES: Record<SettingsTab, ComponentType> = {
  general: General,
  system: System,
  notetaker: NotetakerSettings,
  "vibe-coding": VibeCoding,
  connectors: Connectors,
  mcp: Mcp,
  account: Account,
  billing: About,
  privacy: Privacy,
};
