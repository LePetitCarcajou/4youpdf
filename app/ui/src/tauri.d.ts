// The subset of Tauri's global API the interface uses. Tauri injects
// `window.__TAURI__` because `app.withGlobalTauri` is set in
// tauri.conf.json; no npm package is needed. See
// https://v2.tauri.app/reference/javascript/api/ for the full surface.

interface TauriDragDropPayload {
  type: "enter" | "over" | "drop" | "leave";
  paths?: string[];
  position?: { x: number; y: number };
}

interface TauriEvent<T> {
  event: string;
  id: number;
  payload: T;
}

interface TauriWebview {
  onDragDropEvent(
    handler: (event: TauriEvent<TauriDragDropPayload>) => void,
  ): Promise<() => void>;
}

interface TauriWindow {
  setTitle(title: string): Promise<void>;
}

interface TauriGlobal {
  core: {
    invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  };
  event: {
    listen<T>(
      name: string,
      handler: (event: TauriEvent<T>) => void,
    ): Promise<() => void>;
  };
  webview: {
    getCurrentWebview(): TauriWebview;
  };
  window: {
    getCurrentWindow(): TauriWindow;
  };
}

interface Window {
  __TAURI__?: TauriGlobal;
}
