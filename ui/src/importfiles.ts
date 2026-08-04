// Pure logic for importing outside files into the vault (a drag from Finder onto the
// file tree, and the Import files… picker) — no DOM, no IPC — so node runs its test
// straight off the source (`npm test`), like newentry.ts / move.ts. main.ts owns the
// DragEvent plumbing; the rules about *what* may be imported, and what to say about it,
// live here where they can be tested.
//
// Why a size limit exists at all, and why it's the frontend's. A dropped file reaches
// the webview as **content**, not a path (WebKit hands the page bytes; only Tauri's own
// drag-drop channel carries paths, and that channel is off — see main.ts's
// `dragDropEnabled: false` note). So a drop has to read the whole file into memory and
// base64 it across the IPC, and the peak cost is a couple of multiples of the file. A
// 4 GB video dropped by accident would hang the window; refusing it up front — the size
// is known before a byte is read — turns that into one sentence naming the way through
// (Finder-copy into the vault folder, which the fs watcher picks up anyway). The host
// enforces no such limit: `Vault::import_file` takes the bytes it is given, and the
// picker path (real paths, no byte transport) is deliberately not capped here either.

/** The most a *dropped* file may weigh, in bytes. See the module note. */
export const IMPORT_SIZE_LIMIT = 64 * 1024 * 1024;

/** What main.ts knows about one dropped entry before reading it. */
export interface ImportCandidate {
  name: string;
  size: number;
  isDirectory: boolean;
}

/** The verdict on a drop: what to send, and a phrased reason per thing refused. */
export interface ImportPlan<T> {
  accepted: T[];
  refused: string[];
}

/** A byte count as a short human size — "8 bytes", "1.4 MB". */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} byte${bytes === 1 ? "" : "s"}`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

/**
 * Split a drop into what will be sent and what won't, with the reason already
 * phrased for the toast. Generic over the caller's entry type so the `File` handle
 * rides along with the metadata — the plan is a filter, not a projection.
 *
 * Two refusals, both decidable without reading a byte: a **folder** (a drop of one
 * arrives as a directory entry with no content to send — recursing into it is a
 * bigger feature than this gesture, and saying so beats importing nothing silently)
 * and a file over [`IMPORT_SIZE_LIMIT`].
 */
export function planImport<T extends ImportCandidate>(entries: T[]): ImportPlan<T> {
  const accepted: T[] = [];
  const refused: string[] = [];
  for (const entry of entries) {
    if (entry.isDirectory) {
      refused.push(`${entry.name} is a folder — drop the files inside it`);
    } else if (entry.size > IMPORT_SIZE_LIMIT) {
      refused.push(
        `${entry.name} is too large to drop (${formatSize(entry.size)}, over the ${formatSize(
          IMPORT_SIZE_LIMIT,
        )} limit) — copy it into the vault folder instead`,
      );
    } else {
      accepted.push(entry);
    }
  }
  return { accepted, refused };
}

/** How the toast names a destination folder — the root has no path to say. */
export function destinationLabel(dir: string): string {
  return dir ? `${dir}/` : "the vault root";
}

/**
 * The one sentence (or two) an import reports. Says what landed first — that is the
 * outcome the user asked for — then everything that didn't, each with its reason.
 * Refusals are never summarized into a count: "1 skipped" teaches nothing, and the
 * whole point of refusing early is to be able to say why.
 */
export function importSummary(dir: string, imported: string[], refused: string[]): string {
  const where = destinationLabel(dir);
  const landed =
    imported.length === 0
      ? ""
      : imported.length === 1
        ? `Imported ${imported[0]}.`
        : `Imported ${imported.length} files into ${where}.`;
  if (refused.length === 0) return landed || `Nothing to import into ${where}.`;
  const skipped = refused.join("; ");
  return landed ? `${landed} Skipped: ${skipped}.` : `Nothing imported. ${skipped}.`;
}

/**
 * Bytes → base64, the shape the `import_file` command decodes (transport only; see
 * that command's doc for why the drop path can't hand over a path instead).
 * Chunked because `String.fromCharCode(...bytes)` spreads one argument per byte and
 * a whole-file spread blows the argument limit somewhere in the low hundreds of KB —
 * i.e. it would work in every test and fail on the first real PDF.
 */
export function bytesToBase64(bytes: Uint8Array): string {
  const CHUNK = 0x8000;
  let binary = "";
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}
