// The shortcuts of the browser that the interface neutralizes, as data:
// `main.ts` prevents their default before any other listener sees them, and
// `tests/shortcuts.test.ts` checks the list without a DOM.
//
// wry leaves the shortcuts of WebView2 on: F5 reloads the page of the
// interface, and the open document goes with its undo history, without a
// word; Ctrl+P prints the interface. wry can turn them all off
// (`with_browser_accelerator_keys`), but Tauri 2.11 does not pass that on,
// and reaching `AreBrowserAcceleratorKeysEnabled` otherwise would take an
// `unsafe` COM call. WebView2 does not act on a keydown whose default the
// page prevents: measured with F5, F12 and F3 sent to it as window messages;
// combinations with Ctrl or Alt could not be sent that way (app/README.md,
// « Raccourcis du navigateur neutralisés »).
//
// Neutralized is not given up. Only the default of the browser is
// prevented: each key still reaches the listeners of the window, free for a
// command of the application, and some already wait for one
// (docs/backlog-ui.md). The shortcuts of DevTools, and the keys a listener
// of Tauri takes for one of them, stop there (`stopsHere`).

/// What a keyboard event says about a key: its virtual-key code, its place
/// on a US keyboard, and the modifiers held with it.
export interface KeyPress {
  readonly keyCode: number;
  readonly code: string;
  readonly ctrlKey: boolean;
  readonly shiftKey: boolean;
  readonly altKey: boolean;
  readonly metaKey: boolean;
}

/// What WebView2 does with a shortcut.
export type BrowserAction = "reload" | "print" | "find" | "zoom" | "history" | "caret" | "downloads" | "devtools";

/// A shortcut of the browser: a virtual-key code with exactly these
/// modifiers, so that the same key with other modifiers stays free (R, Maj+R,
/// AltGr, which Windows reports as Ctrl+Alt).
export interface BrowserShortcut {
  readonly action: BrowserAction;
  readonly keyCode: number;
  readonly modifiers: "" | "ctrl" | "shift" | "alt" | "ctrl+shift";
}

/// Virtual-key codes of Windows. Chromium, which WebView2 is built on,
/// recognizes its shortcuts by them, and `keyCode` reports them: `key` would
/// miss the shortcuts on a non-Latin layout, and `code`, a position on a US
/// keyboard, would take other keys for them on a Bépo or Dvorak layout.
const VK = {
  LEFT: 0x25,
  RIGHT: 0x27,
  DIGIT_0: 0x30,
  C: 0x43,
  F: 0x46,
  G: 0x47,
  I: 0x49,
  J: 0x4a,
  P: 0x50,
  R: 0x52,
  NUMPAD_0: 0x60,
  ADD: 0x6b,
  SUBTRACT: 0x6d,
  F3: 0x72,
  F5: 0x74,
  F7: 0x76,
  F12: 0x7b,
  BROWSER_BACK: 0xa6,
  BROWSER_FORWARD: 0xa7,
  BROWSER_REFRESH: 0xa8,
  PLUS: 0xbb,
  MINUS: 0xbd,
} as const;

/// The shortcuts WebView2 acts on while its browser shortcuts are on: the
/// table of its documentation (« Differences between Microsoft Edge and
/// WebView2 »), its Ctrl++, Ctrl+- and Ctrl+0 in the forms Chromium binds
/// (with Shift, on the keypad); plus F12, which the reference of
/// `AreBrowserAcceleratorKeysEnabled` names outside that table, and
/// Ctrl+Shift+P, which prints in Chromium; Escape left out: the page view
/// uses it, and the browser only stops a loading page with it.
export const BROWSER_SHORTCUTS: readonly BrowserShortcut[] = [
  // Reload, and reload bypassing the cache.
  { action: "reload", keyCode: VK.F5, modifiers: "" },
  { action: "reload", keyCode: VK.F5, modifiers: "shift" },
  { action: "reload", keyCode: VK.F5, modifiers: "ctrl" },
  { action: "reload", keyCode: VK.R, modifiers: "ctrl" },
  { action: "reload", keyCode: VK.R, modifiers: "ctrl+shift" },
  { action: "reload", keyCode: VK.BROWSER_REFRESH, modifiers: "" },
  { action: "reload", keyCode: VK.BROWSER_REFRESH, modifiers: "shift" },
  { action: "reload", keyCode: VK.BROWSER_REFRESH, modifiers: "ctrl" },
  // Print; with Shift, through the dialog of the system.
  { action: "print", keyCode: VK.P, modifiers: "ctrl" },
  { action: "print", keyCode: VK.P, modifiers: "ctrl+shift" },
  // Find in the page, next, previous.
  { action: "find", keyCode: VK.F, modifiers: "ctrl" },
  { action: "find", keyCode: VK.G, modifiers: "ctrl" },
  { action: "find", keyCode: VK.G, modifiers: "ctrl+shift" },
  { action: "find", keyCode: VK.F3, modifiers: "" },
  { action: "find", keyCode: VK.F3, modifiers: "shift" },
  // Zoom in, out, back to 100 %. The configuration already keeps the zoom of
  // WebView2 off, keys and wheel (app/src/main.rs, tests): the keys stay here
  // in case it changes, the wheel is left to it.
  { action: "zoom", keyCode: VK.PLUS, modifiers: "ctrl" },
  { action: "zoom", keyCode: VK.PLUS, modifiers: "ctrl+shift" },
  { action: "zoom", keyCode: VK.ADD, modifiers: "ctrl" },
  { action: "zoom", keyCode: VK.MINUS, modifiers: "ctrl" },
  { action: "zoom", keyCode: VK.MINUS, modifiers: "ctrl+shift" },
  { action: "zoom", keyCode: VK.SUBTRACT, modifiers: "ctrl" },
  { action: "zoom", keyCode: VK.DIGIT_0, modifiers: "ctrl" },
  { action: "zoom", keyCode: VK.NUMPAD_0, modifiers: "ctrl" },
  // Back and forward in the history of the page.
  { action: "history", keyCode: VK.LEFT, modifiers: "alt" },
  { action: "history", keyCode: VK.RIGHT, modifiers: "alt" },
  { action: "history", keyCode: VK.BROWSER_BACK, modifiers: "" },
  { action: "history", keyCode: VK.BROWSER_FORWARD, modifiers: "" },
  // Caret browsing.
  { action: "caret", keyCode: VK.F7, modifiers: "" },
  // Downloads.
  { action: "downloads", keyCode: VK.J, modifiers: "ctrl" },
  // DevTools, in every build: the build tried must be the build shipped,
  // where they are off. The context menu and the debugging port still open
  // them in a development build.
  { action: "devtools", keyCode: VK.F12, modifiers: "" },
  { action: "devtools", keyCode: VK.I, modifiers: "ctrl+shift" },
  { action: "devtools", keyCode: VK.J, modifiers: "ctrl+shift" },
  { action: "devtools", keyCode: VK.C, modifiers: "ctrl+shift" },
];

/// The shortcut of the browser `key` is, if any.
export function browserShortcut(key: KeyPress): BrowserShortcut | undefined {
  const held = modifiers(key);
  return BROWSER_SHORTCUTS.find((s) => s.keyCode === key.keyCode && s.modifiers === held);
}

/// Whether `key` goes no further than the guard: a shortcut of DevTools, or a
/// key that the listener Tauri adds to the page in a development build
/// (`toggle-devtools.js`) takes for Ctrl+Shift+I. That listener opens DevTools
/// whatever the default, on Ctrl and Shift held with the key at the place of
/// I on a US keyboard, whatever else is held and whatever the layout types:
/// AltGr+Shift+I, Win+Ctrl+Shift+I, Ctrl+Shift+D on a Bépo layout. None of
/// these is a command of the application, and stopping a key does not
/// prevent what it types.
export function stopsHere(key: KeyPress): boolean {
  return browserShortcut(key)?.action === "devtools" || (key.ctrlKey && key.shiftKey && key.code === "KeyI");
}

/// The modifiers held with `key`, in the order of `BrowserShortcut`.
function modifiers(key: KeyPress): string {
  const names: string[] = [];
  if (key.ctrlKey) {
    names.push("ctrl");
  }
  if (key.shiftKey) {
    names.push("shift");
  }
  if (key.altKey) {
    names.push("alt");
  }
  if (key.metaKey) {
    names.push("meta");
  }
  return names.join("+");
}
