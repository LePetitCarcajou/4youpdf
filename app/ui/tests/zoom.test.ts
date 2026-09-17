// The zoom of the page view: its levels up to the ceiling the renderer
// sets, the point under the pointer that stays put, the bounds a page is
// moved within, the edges that turn the page, the wheel travel that counts
// as a level, and the keys.

import { type KeyPress } from "../src/shortcuts.js";
import {
  arriveAt,
  atEdge,
  clampOffset,
  fit,
  inner,
  maxZoom,
  overflows,
  scaled,
  zoomAround,
  zoomIn,
  zoomKey,
  zoomLabel,
  zoomLevels,
  zoomOut,
  zoomTicks,
  type Room,
  type ZoomKeyPress,
} from "../src/zoom.js";
import { equal, run, test } from "./check.js";

/// A stage 1000 by 800, with the buttons at the sides and the margin below
/// that the fitted page keeps clear: 872 by 776 inside.
const ROOM: Room = { width: 1000, height: 800, left: 64, top: 4, right: 64, bottom: 20 };

/// The rounding `clampOffset` may leave (a half pixel when centring).
function near(actual: number, expected: number): boolean {
  return Math.abs(actual - expected) <= 0.5;
}

test("the ceiling is where the image is as wide as the renderer draws, and never below the fitted page", () => {
  equal(maxZoom(1024, 1), 4, "a page fitted 1024 pixels wide, on a 1:1 screen");
  equal(maxZoom(1024, 2), 2, "the same page on a screen of two device pixels per CSS pixel");
  equal(maxZoom(4096, 1), 1, "a page fitted as wide as the renderer draws");
  equal(maxZoom(5000, 1), 1, "a page fitted wider than that is enlarged already: no zoom");
});

test("the levels are the fitted page, the steps under the ceiling, then the ceiling", () => {
  equal(zoomLevels(4.63), [1, 1.25, 1.5, 2, 3, 4, 4.63], "a ceiling well above the last step");
  equal(zoomLevels(4.2), [1, 1.25, 1.5, 2, 3, 4.2], "a step within 10 % of the ceiling is dropped for it");
  equal(zoomLevels(1.45), [1, 1.25, 1.45], "a screen of many device pixels: two levels");
  equal(zoomLevels(1.05), [1], "a ceiling within 10 % of the fitted page: no zoom");
  equal(zoomLevels(1), [1], "no room to zoom");
  equal(zoomLevels(20), [1, 1.25, 1.5, 2, 3, 4, 6, 8, 12, 16, 20], "a small window: the whole ladder");
});

test("a step goes to the next level, and stays at the ends", () => {
  const levels = zoomLevels(4.63);
  equal(zoomIn(1, levels), 1.25, "in from the fitted page");
  equal(zoomIn(4, levels), 4.63, "in to the ceiling");
  equal(zoomIn(4.63, levels), 4.63, "in at the ceiling: nothing");
  equal(zoomOut(4.63, levels), 4, "out from the ceiling");
  equal(zoomOut(1.25, levels), 1, "out to the fitted page");
  equal(zoomOut(1, levels), 1, "out at the fitted page: nothing");
  // The window grew: the zoom kept is no longer a level of the new ceiling.
  const grown = zoomLevels(3.2);
  equal(grown, [1, 1.25, 1.5, 2, 3.2], "3 dropped for a ceiling of 3.2");
  equal(zoomIn(3, grown), 3.2, "in from a zoom between two levels");
  equal(zoomOut(3, grown), 2, "out from a zoom between two levels");
});

test("the fitted page, and its sizes when enlarged", () => {
  equal(inner(ROOM), { width: 872, height: 776 }, "the padded box");
  const a4 = 842 / 595;
  equal(fit(inner(ROOM), a4), { width: 548, height: 775 }, "A4 fitted by height");
  equal(fit(inner(ROOM), 0.5), { width: 872, height: 436 }, "a wide page fitted by width");
  equal(scaled({ width: 548, height: 775 }, 2), { width: 1096, height: 1550 }, "twice");
  equal(scaled({ width: 548, height: 775 }, 1.25), { width: 685, height: 969 }, "rounded");
});

test("zooming keeps the point of the page under the pointer where it is", () => {
  const pointer = { x: 300, y: 200 };
  const offset = { x: 100, y: 50 };
  const size = { width: 400, height: 600 };
  const resized = { width: 800, height: 1200 };
  const moved = zoomAround(pointer, offset, size, resized);
  equal(moved, { x: -100, y: -100 }, "the page grows away from the pointer");
  equal(
    [(pointer.x - moved.x) / resized.width, (pointer.y - moved.y) / resized.height],
    [(pointer.x - offset.x) / size.width, (pointer.y - offset.y) / size.height],
    "the same point of the page is under the pointer",
  );
  equal(zoomAround(pointer, offset, size, size), offset, "no change of size: no move");
  equal(
    zoomAround({ x: 0, y: 0 }, offset, size, resized),
    { x: 200, y: 100 },
    "a pointer off the page, on the ground: the page still grows away from it",
  );
});

test("a page that fits is centred in the padded box, a larger one moves as far as its edge", () => {
  const fits = clampOffset({ x: -500, y: 999 }, { width: 600, height: 700 }, ROOM);
  equal([near(fits.x, 200), near(fits.y, 42)], [true, true], `centred: ${JSON.stringify(fits)}`);
  const wide = { width: 2000, height: 700 };
  equal(clampOffset({ x: 5, y: 0 }, wide, ROOM).x, 0, "not past its left edge");
  equal(clampOffset({ x: -1500, y: 0 }, wide, ROOM).x, -1000, "not past its right edge");
  equal(clampOffset({ x: -400, y: 0 }, wide, ROOM).x, -400, "anywhere between");
  equal(near(clampOffset({ x: -400, y: 0 }, wide, ROOM).y, 42), true, "still centred on the other axis");
  // Wider than the padded box, not than the room: anywhere within the room.
  const slightly = { width: 900, height: 700 };
  equal(clampOffset({ x: -50, y: 0 }, slightly, ROOM).x, 0, "not past the left of the room");
  equal(clampOffset({ x: 150, y: 0 }, slightly, ROOM).x, 100, "not past the right of the room");
  equal(clampOffset({ x: 60, y: 0 }, slightly, ROOM).x, 60, "between");
  // Infinite offsets go to the edges.
  equal(clampOffset({ x: Number.POSITIVE_INFINITY, y: 0 }, wide, ROOM).x, 0, "to the start");
  equal(clampOffset({ x: Number.NEGATIVE_INFINITY, y: 0 }, wide, ROOM).x, -1000, "to the end");
});

test("a page overflows on the axes where it exceeds the padded box", () => {
  equal(overflows({ width: 872, height: 776 }, ROOM), { x: false, y: false }, "fitted");
  equal(overflows({ width: 873, height: 776 }, ROOM), { x: true, y: false }, "one pixel wider");
  equal(overflows({ width: 500, height: 1200 }, ROOM), { x: false, y: true }, "taller");
  equal(overflows({ width: 2000, height: 2000 }, ROOM), { x: true, y: true }, "both");
});

test("at an edge, moving further would show nothing more", () => {
  const wide = { width: 2000, height: 700 };
  equal(atEdge({ x: 0, y: 0 }, wide, ROOM, "x", -1), true, "at the left edge, going left");
  equal(atEdge({ x: 0, y: 0 }, wide, ROOM, "x", 1), false, "at the left edge, going right");
  equal(atEdge({ x: -1000, y: 0 }, wide, ROOM, "x", 1), true, "at the right edge, going right");
  equal(atEdge({ x: -999.7, y: 0 }, wide, ROOM, "x", 1), true, "within half a pixel of it");
  equal(atEdge({ x: -400, y: 0 }, wide, ROOM, "x", 1), false, "between");
  equal(atEdge({ x: -400, y: 0 }, wide, ROOM, "y", 1), true, "an axis that fits is at both edges");
  equal(atEdge({ x: -400, y: 0 }, wide, ROOM, "y", -1), true, "an axis that fits is at both edges");
});

test("a page entered from an edge arrives by the edge the travel continues through", () => {
  const offset = { x: -400, y: -100 };
  const forward = clampOffset(arriveAt(offset, { axis: "y", edge: "start" }), { width: 2000, height: 2000 }, ROOM);
  equal(forward, { x: -400, y: 0 }, "forward: the top, the other axis kept");
  const back = clampOffset(arriveAt(offset, { axis: "y", edge: "end" }), { width: 2000, height: 2000 }, ROOM);
  equal(back, { x: -400, y: -1200 }, "back: the bottom");
  const right = clampOffset(arriveAt(offset, { axis: "x", edge: "start" }), { width: 2000, height: 2000 }, ROOM);
  equal(right, { x: 0, y: -100 }, "forward on the other axis: the left");
});

test("a wheel gesture zooms one level at 30 pixels, then one more every 100", () => {
  equal([zoomTicks(0), zoomTicks(29), zoomTicks(-29)], [0, 0, 0], "nothing yet");
  equal([zoomTicks(30), zoomTicks(100), zoomTicks(129)], [1, 1, 1], "one level: a notch of a mouse wheel");
  equal([zoomTicks(130), zoomTicks(200), zoomTicks(229)], [2, 2, 2], "two");
  equal([zoomTicks(300), zoomTicks(-300)], [3, 3], "three notches, either way");
});

test("the zoom reads as a percentage of the fitted page", () => {
  equal(zoomLabel(1), "100 %", "fitted, a no-break space before the sign");
  equal(zoomLabel(1.25), "125 %", "a step");
  equal(zoomLabel(4.6296), "463 %", "the ceiling, rounded");
});

/// Virtual-key codes and characters of the keys named in the tests, on a
/// US layout unless the name says AZERTY.
const KEYS: Record<string, { keyCode: number; code: string; key: string; shifted?: string }> = {
  Plus: { keyCode: 0xbb, code: "Equal", key: "=", shifted: "+" },
  Minus: { keyCode: 0xbd, code: "Minus", key: "-", shifted: "_" },
  Add: { keyCode: 0x6b, code: "NumpadAdd", key: "+" },
  Subtract: { keyCode: 0x6d, code: "NumpadSubtract", key: "-" },
  Digit0: { keyCode: 0x30, code: "Digit0", key: "0", shifted: ")" },
  Numpad0: { keyCode: 0x60, code: "Numpad0", key: "0" },
  Digit6: { keyCode: 0x36, code: "Digit6", key: "6", shifted: "^" },
  A: { keyCode: 0x41, code: "KeyA", key: "a", shifted: "A" },
  // The « 6 - » key of an AZERTY keyboard carries the code of 6 and types « - ».
  AzertyMinus: { keyCode: 0x36, code: "Digit6", key: "-", shifted: "6" },
  // Its « à 0 » key carries the code of 0 and types « à », « 0 » with Shift.
  AzertyZero: { keyCode: 0x30, code: "Digit0", key: "à", shifted: "0" },
};

/// The key `spec` names, such as « Ctrl+Shift+Plus » or « AltGr+Digit0 »
/// (Windows reports AltGr as Ctrl+Alt).
function press(spec: string): ZoomKeyPress {
  const parts = spec.split("+");
  const name = parts.pop() ?? "";
  const pressed = KEYS[name];
  if (pressed === undefined) {
    throw new Error(`unknown key « ${name} » in « ${spec} »`);
  }
  const has = (modifier: string): boolean => parts.includes(modifier);
  const base: KeyPress = {
    keyCode: pressed.keyCode,
    code: pressed.code,
    ctrlKey: has("Ctrl") || has("AltGr"),
    shiftKey: has("Shift"),
    altKey: has("Alt") || has("AltGr"),
    metaKey: has("Win"),
  };
  return { ...base, key: base.shiftKey ? (pressed.shifted ?? pressed.key) : pressed.key };
}

test("Ctrl with plus, minus or 0 zooms in, out, or fits the page again", () => {
  for (const spec of ["Ctrl+Plus", "Ctrl+Shift+Plus", "Ctrl+Add"]) {
    equal(zoomKey(press(spec)), "in", spec);
  }
  for (const spec of ["Ctrl+Minus", "Ctrl+Shift+Minus", "Ctrl+Subtract"]) {
    equal(zoomKey(press(spec)), "out", spec);
  }
  for (const spec of ["Ctrl+Digit0", "Ctrl+Numpad0"]) {
    equal(zoomKey(press(spec)), "fit", spec);
  }
});

test("on an AZERTY keyboard, Ctrl with the keys that type - and 0 zoom as well", () => {
  equal(zoomKey(press("Ctrl+AzertyMinus")), "out", "Ctrl with the « 6 - » key, which the browsers take for Ctrl+6");
  equal(zoomKey(press("Ctrl+AzertyZero")), "fit", "Ctrl with the « à 0 » key: the code of 0");
  equal(zoomKey(press("Ctrl+Shift+AzertyZero")), "fit", "Ctrl+Shift with it, which types 0");
});

test("other keys, and the zoom keys with other modifiers, ask nothing of the zoom", () => {
  for (const spec of ["Plus", "Minus", "Digit0", "Shift+Plus", "Ctrl+A", "Ctrl+Digit6", "AltGr+Plus", "AltGr+Digit0", "Ctrl+Alt+Minus", "Win+Ctrl+Plus"]) {
    equal(zoomKey(press(spec)), undefined, spec);
  }
});

await run();
