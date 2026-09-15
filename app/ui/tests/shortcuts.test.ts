// The shortcuts of the browser the interface neutralizes: every one WebView2
// acts on, none of the keys of the application, and none of the same keys
// held with other modifiers.

import { BROWSER_SHORTCUTS, browserShortcut, stopsHere, type BrowserAction, type KeyPress } from "../src/shortcuts.js";
import { equal, run, test } from "./check.js";

/// Virtual-key codes of the keys named in the tests.
const CODES: Record<string, number> = {
  Backspace: 0x08,
  Enter: 0x0d,
  Escape: 0x1b,
  PageUp: 0x21,
  PageDown: 0x22,
  End: 0x23,
  Home: 0x24,
  Left: 0x25,
  Up: 0x26,
  Right: 0x27,
  Down: 0x28,
  Delete: 0x2e,
  A: 0x41,
  C: 0x43,
  D: 0x44,
  E: 0x45,
  F: 0x46,
  G: 0x47,
  I: 0x49,
  J: 0x4a,
  O: 0x4f,
  P: 0x50,
  R: 0x52,
  S: 0x53,
  V: 0x56,
  X: 0x58,
  Y: 0x59,
  Z: 0x5a,
  Add: 0x6b,
  Subtract: 0x6d,
  F3: 0x72,
  F4: 0x73,
  F5: 0x74,
  F7: 0x76,
  F12: 0x7b,
  BrowserBack: 0xa6,
  BrowserForward: 0xa7,
  BrowserRefresh: 0xa8,
  Plus: 0xbb,
  Minus: 0xbd,
};
for (let d = 0; d <= 9; d += 1) {
  CODES[`Digit${d}`] = 0x30 + d;
  CODES[`Numpad${d}`] = 0x60 + d;
}

/// `KeyboardEvent.code` of the keys named in the tests, on a US layout.
const PLACES: Record<string, string> = {
  Left: "ArrowLeft",
  Up: "ArrowUp",
  Right: "ArrowRight",
  Down: "ArrowDown",
  Add: "NumpadAdd",
  Subtract: "NumpadSubtract",
  Plus: "Equal",
  Minus: "Minus",
};

/// The key `spec` names, such as « Ctrl+Shift+R » or « AltGr+Digit0 »
/// (Windows reports AltGr as Ctrl+Alt), on a US layout.
function press(spec: string): KeyPress {
  const parts = spec.split("+");
  const name = parts.pop() ?? "";
  const keyCode = CODES[name];
  if (keyCode === undefined) {
    throw new Error(`unknown key « ${name} » in « ${spec} »`);
  }
  const has = (modifier: string): boolean => parts.includes(modifier);
  return {
    keyCode,
    code: PLACES[name] ?? (/^[A-Z]$/.test(name) ? `Key${name}` : name),
    ctrlKey: has("Ctrl") || has("AltGr"),
    shiftKey: has("Shift"),
    altKey: has("Alt") || has("AltGr"),
    metaKey: has("Win"),
  };
}

function actionOf(spec: string): BrowserAction | null {
  return browserShortcut(press(spec))?.action ?? null;
}

test("every shortcut WebView2 acts on is neutralized, with what it would do", () => {
  const expected: [BrowserAction, string[]][] = [
    ["reload", ["F5", "Shift+F5", "Ctrl+F5", "Ctrl+R", "Ctrl+Shift+R", "BrowserRefresh", "Shift+BrowserRefresh", "Ctrl+BrowserRefresh"]],
    ["print", ["Ctrl+P", "Ctrl+Shift+P"]],
    ["find", ["Ctrl+F", "Ctrl+G", "Ctrl+Shift+G", "F3", "Shift+F3"]],
    ["zoom", ["Ctrl+Plus", "Ctrl+Shift+Plus", "Ctrl+Add", "Ctrl+Minus", "Ctrl+Shift+Minus", "Ctrl+Subtract", "Ctrl+Digit0", "Ctrl+Numpad0"]],
    ["history", ["Alt+Left", "Alt+Right", "BrowserBack", "BrowserForward"]],
    ["caret", ["F7"]],
    ["downloads", ["Ctrl+J"]],
    ["devtools", ["F12", "Ctrl+Shift+I", "Ctrl+Shift+J", "Ctrl+Shift+C"]],
  ];
  let count = 0;
  for (const [action, specs] of expected) {
    for (const spec of specs) {
      equal(actionOf(spec), action, spec);
      count += 1;
    }
  }
  equal(count, BROWSER_SHORTCUTS.length, "no other shortcut in the list");
});

test("the keys of the application are left alone", () => {
  const keys = [
    // Anywhere: open, save; in the grid: undo, redo, select all.
    "Ctrl+O", "Ctrl+S", "Ctrl+Z", "Ctrl+Shift+Z", "Ctrl+Y", "Ctrl+A",
    // Panel of thumbnails; turning pages, in the grid and in the view.
    "F4", "R", "Shift+R",
    // Grid: delete, open a page, move the selection.
    "Delete", "Backspace", "Enter", "Left", "Right", "Shift+Left", "Shift+Right",
    // View: back to the grid, pages.
    "Escape", "PageUp", "PageDown", "Home", "End", "Up", "Down",
    // Text being typed: a page number, a password.
    "Ctrl+C", "Ctrl+V", "Ctrl+X",
  ];
  // The digits of the view, top row (with Shift on an AZERTY keyboard) and keypad.
  for (let d = 0; d <= 9; d += 1) {
    keys.push(`Digit${d}`, `Shift+Digit${d}`, `Numpad${d}`);
  }
  for (const spec of keys) {
    equal(actionOf(spec), null, spec);
  }
});

test("a shortcut held with other modifiers is left alone", () => {
  // AltGr types characters: « @ » (AltGr+0 on an AZERTY keyboard), « } »
  // (AltGr+=), « € » (AltGr+E), in a password as anywhere.
  for (const spec of ["AltGr+Digit0", "AltGr+Plus", "AltGr+E", "AltGr+R", "AltGr+P", "AltGr+F", "AltGr+G", "AltGr+J"]) {
    equal(actionOf(spec), null, spec);
  }
  // Alt with a function key belongs to Windows (Alt+F4 closes the window).
  for (const spec of ["Alt+F4", "Alt+F5", "Alt+F3", "Alt+F7", "Alt+F12"]) {
    equal(actionOf(spec), null, spec);
  }
  for (const spec of ["Win+R", "Win+F5", "Win+F12", "Ctrl+Shift+F5", "Ctrl+Shift+F", "Shift+F7", "Ctrl+Alt+Left", "Shift+Alt+Left", "Ctrl+Left", "Ctrl+F12"]) {
    equal(actionOf(spec), null, spec);
  }
});

test("the shortcuts of DevTools, and what Tauri's listener takes for Ctrl+Shift+I, stop at the guard", () => {
  // In a development build, a listener that Tauri adds to the page opens
  // DevTools on Ctrl and Shift with the key at the place of I, whatever the
  // default, whatever else is held and whatever the layout types.
  for (const spec of ["F12", "Ctrl+Shift+I", "Ctrl+Shift+J", "Ctrl+Shift+C", "AltGr+Shift+I", "Win+Ctrl+Shift+I"]) {
    equal(stopsHere(press(spec)), true, spec);
  }
  const bepo = { ...press("Ctrl+Shift+D"), code: "KeyI" };
  equal(stopsHere(bepo), true, "Ctrl+Shift+D of a Bépo layout, on the key at the place of I");
  equal(browserShortcut(bepo), undefined, "which is no shortcut of the browser: its default stays");
  for (const spec of ["F5", "Ctrl+R", "Ctrl+P", "Ctrl+F", "Alt+Left", "Ctrl+Plus", "Ctrl+I", "Shift+I", "Ctrl+Alt+I", "Ctrl+O", "R"]) {
    equal(stopsHere(press(spec)), false, spec);
  }
  for (const s of BROWSER_SHORTCUTS.filter((s) => s.action !== "devtools")) {
    const key = {
      keyCode: s.keyCode,
      code: "",
      ctrlKey: s.modifiers.includes("ctrl"),
      shiftKey: s.modifiers.includes("shift"),
      altKey: s.modifiers.includes("alt"),
      metaKey: false,
    };
    equal(stopsHere(key), false, `${s.action} ${s.keyCode} ${s.modifiers}`);
  }
});

await run();
