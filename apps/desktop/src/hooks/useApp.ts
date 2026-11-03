import { useSyncExternalStore } from 'react';
import { appStore } from '../stores/app-store';

export function useApp() {
  return useSyncExternalStore(appStore.subscribe, appStore.get);
}
