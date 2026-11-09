export function maySave(
  dirty: boolean,
  busy: boolean,
  composing: boolean,
  blocked: boolean,
): boolean {
  return dirty && !busy && !composing && !blocked;
}

export function isSaved(sameText: boolean, hasRevision: boolean, busy: boolean): boolean {
  return sameText && hasRevision && !busy;
}

export function mayReadRemote(
  visible: boolean,
  onNote: boolean,
  loading: boolean,
  pending: boolean,
  operation: boolean,
  composing: boolean,
  blocked: boolean,
): boolean {
  return visible && onNote && !loading && !pending && !operation && !composing && !blocked;
}

export function mayApplyRemote(
  sameEditor: boolean,
  sameGeneration: boolean,
  sameRevision: boolean,
  loading: boolean,
  pending: boolean,
  operation: boolean,
  composing: boolean,
  blocked: boolean,
): boolean {
  return (
    sameEditor &&
    sameGeneration &&
    sameRevision &&
    !loading &&
    !pending &&
    !operation &&
    !composing &&
    !blocked
  );
}

export function hasRemoteConflict(dirty: boolean, sameText: boolean): boolean {
  return dirty && !sameText;
}

export function mayRecoverDraft(
  matchesNote: boolean,
  legacyOwner: boolean,
  coordinated: boolean,
  activeOwner: boolean,
): boolean {
  return matchesNote && (legacyOwner || (coordinated && !activeOwner));
}
