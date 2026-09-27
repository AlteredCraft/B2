// `turndown-plugin-gfm` ships no types and has no @types package. Each export is a plugin
// that registers its rules on the service.
declare module "turndown-plugin-gfm" {
  import type TurndownService from "turndown";
  type Plugin = (service: TurndownService) => void;
  export const gfm: Plugin;
  export const highlightedCodeBlock: Plugin;
  export const strikethrough: Plugin;
  export const tables: Plugin;
  export const taskListItems: Plugin;
}
