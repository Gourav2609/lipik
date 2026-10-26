import type { ComponentType } from "react";
import type { OverlayId } from "../state";
import type { OverlayProps } from "./types";
import AccountMenu from "./AccountMenu";
import ThemeMenu from "./ThemeMenu";
import Toast from "./Toast";
import Confetti from "./Confetti";
import ConnectCalendar from "./ConnectCalendar";

export type { OverlayProps } from "./types";

export const OVERLAYS: Record<OverlayId, ComponentType<OverlayProps>> = {
  "account-menu": AccountMenu,
  "theme-menu": ThemeMenu,
  toast: Toast,
  confetti: Confetti,
  "connect-calendar": ConnectCalendar,
};
