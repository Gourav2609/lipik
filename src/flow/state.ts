import { create } from "zustand";

export const PAGE_IDS = [
  "dictation",
  "notetaker",
  "insights",
  "dictionary",
  "snippets",
  "style",
  "transforms",
  "scratchpad",
] as const;
export type PageId = (typeof PAGE_IDS)[number];

export const SETTINGS_TABS = [
  "general",
  "system",
  "notetaker",
  "vibe-coding",
  "connectors",
  "mcp",
  "account",
  "billing",
  "privacy",
] as const;
export type SettingsTab = (typeof SETTINGS_TABS)[number];

export const OVERLAY_IDS = [
  "account-menu",
  "theme-menu",
  "toast",
  "confetti",
  "connect-calendar",
] as const;
export type OverlayId = (typeof OVERLAY_IDS)[number];

interface FlowState {
  page: PageId;
  sidebarCollapsed: boolean;
  settingsTab: SettingsTab | null;
  overlay: OverlayId | null;
  setPage: (page: PageId) => void;
  setSidebarCollapsed: (collapsed: boolean) => void;
  toggleSidebar: () => void;
  openSettings: (tab?: SettingsTab) => void;
  closeSettings: () => void;
  openOverlay: (overlay: OverlayId) => void;
  closeOverlay: () => void;
}

const oneOf = <T extends string>(
  list: readonly T[],
  value: string | null,
): T | null => (value && (list as readonly string[]).includes(value) ? (value as T) : null);

const params = new URLSearchParams(window.location.search);

export const useFlowStore = create<FlowState>((set) => ({
  page: oneOf(PAGE_IDS, params.get("page")) ?? "dictation",
  sidebarCollapsed: params.get("sidebar") === "collapsed",
  settingsTab: oneOf(SETTINGS_TABS, params.get("settings")),
  overlay: oneOf(OVERLAY_IDS, params.get("overlay")),
  setPage: (page) => set({ page }),
  setSidebarCollapsed: (sidebarCollapsed) => set({ sidebarCollapsed }),
  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
  openSettings: (tab = "general") => set({ settingsTab: tab, overlay: null }),
  closeSettings: () => set({ settingsTab: null }),
  openOverlay: (overlay) => set({ overlay }),
  closeOverlay: () => set({ overlay: null }),
}));
