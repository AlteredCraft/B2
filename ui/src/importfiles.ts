// Pure rules for importing outside files (a Finder drop onto the tree, or the Import
// files… picker): what may be imported and what to say about it.
//
// A dropped file reaches the webview as content, not a path (Tauri's drag-drop channel is
// off, see main.ts), so it is read whole and base64'd across the IPC; a huge accidental
// drop would hang the window. The size limit is the frontend's alone: the picker sends
// paths and is not capped.

/** The most a dropped file may weigh, in bytes. */
export const IMPORT_SIZE_LIMIT = 64 * 1024 * 1024;

/** What main.ts knows about one dropped entry before reading it. */
export interface ImportCandidate {
  name: string;
  size: number;
  isDirectory: boolean;
}

/** The verdict on a drop: what to send, and a reason per thing refused. */
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
 * Split a drop into what will be sent and what won't, with reasons for the toast. A
 * filter, not a projection, so the caller's `File` handle rides along. Refuses folders
 * (not recursed) and files over [`IMPORT_SIZE_LIMIT`].
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

/** How the toast names a destination folder. */
export function destinationLabel(dir: string): string {
  return dir ? `${dir}/` : "the vault root";
}

/**
 * What an import reports: what landed, then each refusal with its reason (never a bare
 * count, which teaches nothing).
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
 * Bytes → base64 for `import_file`. Chunked: a whole-file `String.fromCharCode(...bytes)`
 * spread blows the argument limit in the low hundreds of KB.
 */
export function bytesToBase64(bytes: Uint8Array): string {
  const CHUNK = 0x8000;
  let binary = "";
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}
