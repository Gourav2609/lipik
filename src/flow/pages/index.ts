import type { ComponentType } from "react";
import type { PageId } from "../state";
import Dictation from "./Dictation";
import Notetaker from "./Notetaker";
import Insights from "./Insights";
import Dictionary from "./Dictionary";
import Snippets from "./Snippets";
import Style from "./Style";
import Transforms from "./Transforms";
import Scratchpad from "./Scratchpad";

export const PAGES: Record<PageId, ComponentType> = {
  dictation: Dictation,
  notetaker: Notetaker,
  insights: Insights,
  dictionary: Dictionary,
  snippets: Snippets,
  style: Style,
  transforms: Transforms,
  scratchpad: Scratchpad,
};
