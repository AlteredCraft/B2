// `turndown-plugin-gfm` ships no types and has no @types package. Declare the two
// exports paste.ts leans on (and the siblings, so a future need is one import away):
// each is a plugin — a function handed the service to register its rules on.
declare module "turndown-plugin-gfm" {
  import type TurndownService from "turndown";
  type Plugin = (service: TurndownService) => void;
  export const gfm: Plugin;
  export const highlightedCodeBlock: Plugin;
  export const strikethrough: Plugin;
  export const tables: Plugin;
  export const taskListItems: Plugin;
}
