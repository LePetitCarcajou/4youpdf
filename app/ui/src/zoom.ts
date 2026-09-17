// The zoom of the page view, as arithmetic: `viewer.ts` applies it to the
// page on screen, and `tests/zoom.test.ts` checks it without a DOM.
//
// The zoom is relative to the page fitted to the window, which is 1: the
// view never shows a page smaller than fitted, and never asks the renderer
// for an image wider than it draws (`MAX_WIDTH`). The largest zoom therefore
// depends on the page and on the window: it is the one where the image,
// `MAX_WIDTH` device pixels wide, is shown pixel for pixel. Between the two,
// fixed steps, and the ceiling is the last of them.
//
// Where the page is shown is an offset: its top-left corner, in CSS pixels
// from the top-left of the room it is shown in. A page that fits the room
// is centred, and a page larger than the room may be moved as far as its
// edge meets the edge of the room, no further.

import { browserShortcut, type KeyPress } from "./shortcuts.js";

/// Widest image the renderer draws (`RenderService::render` clamps to it).
export const MAX_WIDTH = 4096;

/// The zoom steps above the fitted page, as factors of it, up to the
/// ceiling: large enough that a detail grows at each one.
const LADDER: readonly number[] = [1.25, 1.5, 2, 3, 4, 6, 8, 12, 16];

/// A step is only worth taking if it enlarges the page by this much: a
/// step of the ladder closer than that to the ceiling is dropped for it.
const LEAST_STEP = 1.1;

/// Pixels of wheel travel for the first zoom level of a gesture, and for
/// each further one: a notch of a mouse wheel, which Chromium reports as
/// 100 pixels, is one level, whatever the setting of the system; the
/// small, frequent deltas of a touchpad add up.
const ZOOM_FIRST = 30;
export const ZOOM_NOTCH = 100;

export interface Point {
  x: number;
  y: number;
}

export interface Size {
  width: number;
  height: number;
}

/// The room a page is shown in: its box, and the padding inside it that a
/// fitted page keeps clear (the buttons at the sides, the margin below).
export interface Room {
  width: number;
  height: number;
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export type Axis = "x" | "y";

/// A page turned from the edge of the page before it enters by the edge
/// the travel continues through: the start (top, left) going forward, the
/// end going back. The other axis keeps its place.
export interface Arrival {
  axis: Axis;
  edge: "start" | "end";
}

/// The padded box of `room`, where a fitted page goes.
export function inner(room: Room): Size {
  return {
    width: Math.max(0, room.width - room.left - room.right),
    height: Math.max(0, room.height - room.top - room.bottom),
  };
}

/// The largest size of height-over-width `ratio` that fits in `area`.
export function fit(area: Size, ratio: number): Size {
  const width = Math.max(1, Math.floor(Math.min(area.width, area.height / ratio)));
  return { width, height: Math.max(1, Math.floor(width * ratio)) };
}

/// `fitted` enlarged `zoom` times.
export function scaled(fitted: Size, zoom: number): Size {
  return { width: Math.round(fitted.width * zoom), height: Math.round(fitted.height * zoom) };
}

/// The largest zoom for a page fitted `fitWidth` CSS pixels wide on a
/// screen of `dpr` device pixels per CSS pixel: the one where the image is
/// `MAX_WIDTH` device pixels wide and shown pixel for pixel. Never below 1:
/// a page fitted wider than the renderer draws is enlarged already.
export function maxZoom(fitWidth: number, dpr: number): number {
  return Math.max(1, MAX_WIDTH / (fitWidth * dpr));
}

/// The zoom levels under the ceiling `max`: the fitted page, the steps of
/// the ladder that leave at least `LEAST_STEP` to the ceiling, then the
/// ceiling itself, unless it is within `LEAST_STEP` of the fitted page,
/// which leaves no zoom at all.
export function zoomLevels(max: number): number[] {
  const levels = [1];
  for (const level of LADDER) {
    if (level * LEAST_STEP <= max) {
      levels.push(level);
    }
  }
  if (max >= LEAST_STEP) {
    levels.push(max);
  }
  return levels;
}

/// The level above `zoom` among `levels`, ascending; `zoom` itself at the
/// top.
export function zoomIn(zoom: number, levels: readonly number[]): number {
  return levels.find((level) => level > zoom) ?? zoom;
}

/// The level below `zoom` among `levels`, ascending; `zoom` itself at the
/// bottom.
export function zoomOut(zoom: number, levels: readonly number[]): number {
  let below = zoom;
  for (const level of levels) {
    if (level < zoom) {
      below = level;
    }
  }
  return below;
}

/// How many zoom levels a wheel gesture has travelled `travel` pixels for,
/// in either direction: the first at `ZOOM_FIRST`, one more for each
/// `ZOOM_NOTCH` after it.
export function zoomTicks(travel: number): number {
  const distance = Math.abs(travel);
  return distance < ZOOM_FIRST ? 0 : 1 + Math.floor((distance - ZOOM_FIRST) / ZOOM_NOTCH);
}

/// Where a page at `offset` goes when it is resized from `size` to
/// `resized`, so that the point of it under `pointer` stays there; all in
/// the coordinates of the room. The result is not yet within bounds
/// (`clampOffset`).
export function zoomAround(pointer: Point, offset: Point, size: Size, resized: Size): Point {
  return {
    x: pointer.x - ((pointer.x - offset.x) * resized.width) / size.width,
    y: pointer.y - ((pointer.y - offset.y) * resized.height) / size.height,
  };
}

/// Where a page of `size` may be shown in `room`: at `offset` if it can. On
/// each axis, a page that fits the padded box is centred there and cannot
/// be moved; a larger one may be moved as far as its edge meets the edge
/// of the room, no further. An infinite offset goes to the edge.
export function clampOffset(offset: Point, size: Size, room: Room): Point {
  return {
    x: clampAxis(offset.x, size.width, room.width, room.left, room.right),
    y: clampAxis(offset.y, size.height, room.height, room.top, room.bottom),
  };
}

function clampAxis(offset: number, size: number, room: number, before: number, after: number): number {
  const padded = room - before - after;
  if (size <= padded) {
    return before + (padded - size) / 2;
  }
  const slack = room - size;
  return Math.min(Math.max(offset, Math.min(0, slack)), Math.max(0, slack));
}

/// On which axes a page of `size` exceeds the padded box of `room`, and
/// can therefore be moved.
export function overflows(size: Size, room: Room): Record<Axis, boolean> {
  const padded = inner(room);
  return { x: size.width > padded.width, y: size.height > padded.height };
}

/// Whether a page of `size` at `offset` shows its edge in `direction` on
/// `axis` (1 for the end, -1 for the start): moving further that way would
/// show nothing more, and turns the page instead. A page that fits on the
/// axis shows both edges.
export function atEdge(offset: Point, size: Size, room: Room, axis: Axis, direction: number): boolean {
  if (!overflows(size, room)[axis]) {
    return true;
  }
  const at = clampOffset(offset, size, room)[axis];
  const limit = axis === "x" ? room.width - size.width : room.height - size.height;
  return direction > 0 ? at <= Math.min(0, limit) + 0.5 : at >= Math.max(0, limit) - 0.5;
}

/// `offset` with the axis of `arrival` sent to the edge it names, which
/// `clampOffset` then settles.
export function arriveAt(offset: Point, arrival: Arrival): Point {
  const edge = arrival.edge === "start" ? Number.POSITIVE_INFINITY : Number.NEGATIVE_INFINITY;
  return arrival.axis === "x" ? { x: edge, y: offset.y } : { x: offset.x, y: edge };
}

/// The zoom as the view shows it: a percentage of the fitted page.
export function zoomLabel(zoom: number): string {
  return `${Math.round(zoom * 100)} %`;
}

/// What a key asks of the zoom.
export type ZoomCommand = "in" | "out" | "fit";

/// What a keyboard event says about a key, and the character it types.
export interface ZoomKeyPress extends KeyPress {
  readonly key: string;
}

/// The zoom command `key` asks for, if any: Ctrl with plus, minus or 0 as
/// the browsers bind them (`shortcuts.ts`, action `zoom`), which the guard
/// neutralizes and leaves to the application; or Ctrl with a key that
/// types « + », « = », « - » or « 0 » on the layout in use. On an AZERTY
/// keyboard the « 6 - » key of the top row carries the code of 6, which
/// the browsers take for Ctrl+6: here it zooms out all the same. AltGr,
/// which Windows reports as Ctrl+Alt, types as usual.
export function zoomKey(key: ZoomKeyPress): ZoomCommand | undefined {
  if (!key.ctrlKey || key.altKey || key.metaKey) {
    return undefined;
  }
  const shortcut = browserShortcut(key);
  if (shortcut?.action === "zoom") {
    return shortcut.zoom === "reset" ? "fit" : shortcut.zoom;
  }
  switch (key.key) {
    case "+":
    case "=":
      return "in";
    case "-":
      return "out";
    case "0":
      return "fit";
    default:
      return undefined;
  }
}
