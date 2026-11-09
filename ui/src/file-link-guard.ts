import type { Snapshot } from "./features.js";
import { changedFileLinks, repairFileLinks } from "./file-links.js";
import { parseAttachment } from "./media.js";

interface FileLinkPort {
  state(): Snapshot | null;
  api(path: string): Promise<unknown>;
  replace(text: string): void;
}

export interface FileLinkGuard {
  changed(state: Snapshot): void;
  beforeSave(identity: string, text: string): Promise<void>;
}

function element<T extends HTMLElement>(id: string, kind: { new (): T }): T {
  const node = document.getElementById(id);
  if (!(node instanceof kind)) throw new Error(`Missing file link control: ${id}`);
  return node;
}

export function initFileLinkGuard(port: FileLinkPort): FileLinkGuard {
  const warning = element("file-link-warning", HTMLDivElement);
  const message = element("file-link-message", HTMLParagraphElement);
  const repair = element("repair-file-links", HTMLButtonElement);
  const baselines = new Map<string, string>();
  let checked: { identity: string; text: string; promise: Promise<void> } | null = null;
  function current(identity: string, text: string): boolean {
    const state = port.state();
    return state?.identity === identity && state.body === text;
  }
  function check(identity: string, text: string): Promise<void> {
    if (checked?.identity === identity && checked.text === text) return checked.promise;
    const baseline = baselines.get(identity);
    const changes = changedFileLinks(baseline ?? text, text);
    if (!changes.length) {
      checked = null;
      if (baseline === undefined || port.state()?.savedBody === text) baselines.set(identity, text);
      if (current(identity, text)) warning.hidden = true;
      return Promise.resolve();
    }
    if (current(identity, text)) {
      warning.hidden = false;
      message.textContent = "Checking a changed file link. Original files remain in note history.";
    }
    const promise = Promise.all(
      changes.map(async (change) => {
        if (change.incomplete) return false;
        const id = /(?:^|\/)\.attachments\/([^/#?]+)(?:[#?].*)?$/.exec(change.destination)?.[1];
        if (!id) return false;
        try {
          const target = decodeURIComponent(id);
          return (
            parseAttachment(await port.api(`/api/attachments/${encodeURIComponent(target)}`)).id ===
            target
          );
        } catch {
          return false;
        }
      }),
    ).then((valid) => {
      if (!current(identity, text)) return;
      if (valid.every(Boolean)) {
        baselines.set(identity, text);
        warning.hidden = true;
      } else {
        message.textContent =
          "A file link cannot be opened. Repair the link or undo the edit. Original files remain in note history.";
      }
    });
    checked = { identity, text, promise };
    return promise;
  }
  repair.addEventListener("click", () => {
    const state = port.state();
    if (!state || state.composing || state.busy) return;
    port.replace(
      repairFileLinks(
        state.body,
        changedFileLinks(baselines.get(state.identity) ?? state.body, state.body),
      ),
    );
  });
  return {
    changed(state) {
      void check(state.identity, state.body);
      repair.disabled = state.busy || state.composing;
    },
    beforeSave: check,
  };
}
