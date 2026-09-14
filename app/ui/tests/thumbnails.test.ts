// How many thumbnails are drawn at once: the renderer serves one request at
// a time, and the page view goes first.

import { thumbnailSlots } from "../src/thumbnails.js";
import { equal, run, test } from "./check.js";

test("three at once with the grid on screen, whatever the view was doing", () => {
  equal(thumbnailSlots({ open: false, panel: true, drawing: false }), 3, "grid");
  equal(thumbnailSlots({ open: false, panel: false, drawing: true }), 3, "grid, a page of the view still being drawn");
});

test("with the page view open, one for the panel while the view draws nothing, none otherwise", () => {
  equal(thumbnailSlots({ open: true, panel: true, drawing: false }), 1, "panel shown, nothing drawn");
  equal(thumbnailSlots({ open: true, panel: true, drawing: true }), 0, "panel shown, a page being drawn");
  equal(thumbnailSlots({ open: true, panel: false, drawing: false }), 0, "panel hidden");
});

await run();
