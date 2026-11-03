import { api, desktopAvailable, emptySnapshot, listenForUpdates } from '../services/api';
import type { AppSnapshot } from '../types';

interface State {
  snapshot: AppSnapshot;
  loading: boolean;
  busy: string | null;
  error: string | null;
}
let state: State = { snapshot: emptySnapshot, loading: desktopAvailable, busy: null, error: null };
const subscribers = new Set<() => void>();
function publish(patch: Partial<State>) {
  state = { ...state, ...patch };
  subscribers.forEach((callback) => callback());
}
export const appStore = {
  get: () => state,
  subscribe(callback: () => void) {
    subscribers.add(callback);
    return () => {
      subscribers.delete(callback);
    };
  },
  clearError() {
    publish({ error: null });
  },
};
let refreshing: Promise<void> | null = null;
let refreshQueued = false;
export function refresh(): Promise<void> {
  if (!desktopAvailable) return Promise.resolve();
  if (refreshing) {
    // An event during a read must not disappear behind an older snapshot.
    refreshQueued = true;
    return refreshing;
  }
  refreshing = (async () => {
    do {
      refreshQueued = false;
      try {
        const snapshot = await api.snapshot();
        publish({ snapshot, loading: false });
      } catch (error) {
        publish({ loading: false, error: String(error) });
      }
    } while (refreshQueued);
  })().finally(() => {
    refreshing = null;
  });
  return refreshing;
}
export async function action(label: string, task: () => Promise<void>): Promise<boolean> {
  if (state.busy) return false;
  publish({ busy: label, error: null });
  try {
    await task();
    await refresh();
    return true;
  } catch (error) {
    publish({ error: String(error) });
    return false;
  } finally {
    publish({ busy: null });
  }
}
export function connect(): () => void {
  let active = true;
  const unlisten = listenForUpdates(() => {
    void refresh();
  });
  unlisten.catch((error) => {
    if (active) publish({ error: String(error) });
  });
  void refresh();
  return () => {
    active = false;
    void unlisten.then((stop) => stop()).catch(() => {});
  };
}
