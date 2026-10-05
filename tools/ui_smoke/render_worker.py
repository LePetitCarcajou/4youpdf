#!/usr/bin/env python3
"""Smoke test of the rendering worker under the real window (palier v0.5.1,
sessions A and B; ADR 0008), driven through the DevTools port of WebView2
(CDP) and the process list of Windows.

What runs for real: the window, its `render_page` and `renderer_status`
commands, the worker process it starts (`fyp-app.exe --fyp-render-worker`)
and PDFium. The worker is ended from here with `TerminateProcess`; the
window is closed with `WM_CLOSE`, as the cross does, then killed, as a crash
would, once while its worker is idle and once while it draws a page that
takes minutes: the job the worker is in ends it at once. Pages are asked
for through `window.__TAURI__.core.invoke` and by the interface itself
(the thumbnails of the grid, the page view). DOM state and process lists
only: nothing here looks at the screen.

Needs PDFium (`python tools/fetch_pdfium.py`) and a development build that
embeds the current interface:

    python tools/build_ui.py
    cargo build -p fyp-app
    python tools/ui_smoke/render_worker.py [path/to/fyp-app.exe]

No other 4YouPDF may run: instances share the WebView2 profile. Windows
only. Exit status 1 when a check fails.
"""

import ctypes
import ctypes.wintypes as wt
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True  # no __pycache__ left in the repository
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import merge_pages as base  # noqa: E402  (the CDP client and the launcher)

FIXTURE = os.path.join(base.FIXTURES, "mixed12.pdf")
ARGUMENT = "--fyp-render-worker"
WM_CLOSE = 0x0010
PROCESS_TERMINATE = 0x0001
NL = chr(10)
user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32

check = base.check


def heavy_pdf(path):
    """A two-page A4 PDF whose first page takes PDFium a while: 40 000 filled curves."""
    lines = []
    for i in range(40000):
        x, y = i * 7 % 590, i * 13 % 840
        lines.append(
            f"{i % 10 / 10:.2f} {i % 7 / 7:.3f} {i % 3 / 3:.3f} rg {x} {y} m "
            f"{x + 200} {y + 50} {x + 50} {y + 300} {x + 5} {y + 5} c f"
        )
    heavy = NL.join(lines)
    light = "0 0 100 100 re f"
    page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents {} 0 R >>"
    objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        page.format(5),
        page.format(6),
        f"<< /Length {len(heavy)} >>{NL}stream{NL}{heavy}{NL}endstream",
        f"<< /Length {len(light)} >>{NL}stream{NL}{light}{NL}endstream",
    ]
    out = f"%PDF-1.7{NL}"
    offsets = []
    for number, body in enumerate(objects, 1):
        offsets.append(len(out))
        out += f"{number} 0 obj{NL}{body}{NL}endobj{NL}"
    xref = len(out)
    out += f"xref{NL}0 {len(objects) + 1}{NL}0000000000 65535 f {NL}"
    for offset in offsets:
        out += f"{offset:010} 00000 n {NL}"
    out += f"trailer{NL}<< /Size {len(objects) + 1} /Root 1 0 R >>{NL}startxref{NL}{xref}{NL}%%EOF{NL}"
    with open(path, "wb") as f:
        f.write(out.encode("ascii"))
    return path


def endless_pdf(path):
    """A one-page A4 PDF that takes PDFium minutes: a form of a thousand
    filled curves, drawn by a form a thousand times."""
    curves = NL.join(
        f"{i * 7 % 590} {i * 13 % 840} m {i * 7 % 590 + 200} {i * 13 % 840 + 50} "
        f"{i * 7 % 590 + 50} {i * 13 % 840 + 300} {i * 7 % 590 + 5} {i * 13 % 840 + 5} c f"
        for i in range(1000)
    )
    calls = NL.join(["/F1 Do"] * 1000)

    def form(resources, content):
        return (
            f"<< /Type /XObject /Subtype /Form /BBox [0 0 595 842] {resources}/Length {len(content)} >>"
            f"{NL}stream{NL}{content}{NL}endstream"
        )

    objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R "
        "/Resources << /XObject << /F1 5 0 R >> >> >>",
        f"<< /Length 6 >>{NL}stream{NL}/F1 Do{NL}endstream",
        form("/Resources << /XObject << /F1 6 0 R >> >> ", calls),
        form("", curves),
    ]
    out = f"%PDF-1.7{NL}"
    offsets = []
    for number, body in enumerate(objects, 1):
        offsets.append(len(out))
        out += f"{number} 0 obj{NL}{body}{NL}endobj{NL}"
    xref = len(out)
    out += f"xref{NL}0 {len(objects) + 1}{NL}0000000000 65535 f {NL}"
    for offset in offsets:
        out += f"{offset:010} 00000 n {NL}"
    out += f"trailer{NL}<< /Size {len(objects) + 1} /Root 1 0 R >>{NL}startxref{NL}{xref}{NL}%%EOF{NL}"
    with open(path, "wb") as f:
        f.write(out.encode("ascii"))
    return path


def powershell(command):
    done = subprocess.run(["powershell", "-NoProfile", "-Command", command], capture_output=True, text=True)
    return done.stdout.strip()


def app_processes():
    """Every `fyp-app.exe` process: pid, parent pid, command line."""
    out = powershell(
        "Get-CimInstance Win32_Process -Filter \"Name='fyp-app.exe'\" | "
        "Select-Object ProcessId,ParentProcessId,CommandLine | ConvertTo-Json -Compress"
    )
    if not out:
        return []
    found = json.loads(out)
    return found if isinstance(found, list) else [found]


def workers(parent):
    return [p for p in app_processes() if p["ParentProcessId"] == parent]


def kill(pid):
    """End process `pid` at once, and wait until it is gone."""
    handle = kernel32.OpenProcess(PROCESS_TERMINATE, False, pid)
    if handle:
        kernel32.TerminateProcess(handle, 1)
        kernel32.CloseHandle(handle)
    end = time.time() + 10
    while time.time() < end and any(p["ProcessId"] == pid for p in app_processes()):
        time.sleep(0.1)


def wait_until(what, predicate, timeout=20.0):
    end = time.time() + timeout
    while True:
        value = predicate()
        if value:
            return value
        if time.time() > end:
            raise RuntimeError(f"timed out waiting for {what}")
        time.sleep(0.1)


def render(page, number, width):
    """Ask the Rust side for a page; ['ok', length of the URL] or ['error', message]."""
    return page.js(
        f"window.__TAURI__.core.invoke('render_page', {{page: {number}, width: {width}}})"
        ".then((url) => ['ok', url.startsWith('data:image/png;base64,') ? url.length : -1],"
        " (e) => ['error', (e && e.message) || String(e)])",
        wait=True,
    )


def tiles(page):
    return page.js(
        "(() => { const all = [...document.querySelectorAll('#grid .tile')]; return {"
        "count: all.length, loaded: all.filter((t) => t.classList.contains('loaded')).length,"
        "failed: all.filter((t) => t.classList.contains('failed')).map((t) => t.title)}; })()"
    )


def status_bar(page):
    return page.js("document.querySelector('#renderer-status').textContent")


def main_window(pid):
    """The visible top-level window of process `pid` whose title starts with 4YouPDF."""
    found = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def each(hwnd, _):
        owner = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(hwnd):
            title = ctypes.create_unicode_buffer(256)
            user32.GetWindowTextW(hwnd, title, 256)
            if title.value.startswith("4YouPDF"):
                found.append(hwnd)
        return True

    user32.EnumWindows(each, 0)
    return found[0] if found else None


def one_worker(proc, what):
    mine = workers(proc.pid)
    check(f"{what}: one worker process, child of the window", len(mine) == 1, json.dumps(mine))
    return mine[0] if mine else None


def main():
    if not os.path.exists(base.EXE):
        sys.exit(f"{base.EXE} missing: python tools/build_ui.py, then cargo build -p fyp-app")
    if app_processes():
        sys.exit("a 4YouPDF already runs: close it first")
    tmp = tempfile.mkdtemp(prefix="fyp-render-worker-")
    heavy = heavy_pdf(os.path.join(tmp, "heavy.pdf"))

    # -- The worker serves the window, dies, and is started again ------------
    proc, page = base.launch(heavy)
    try:
        wait_until("the thumbnails", lambda: tiles(page)["loaded"] == 2, timeout=60)
        seen = tiles(page)
        check("the thumbnails of the grid are drawn through the worker", seen["count"] == 2 and not seen["failed"], json.dumps(seen))
        check("the status bar names the renderer", status_bar(page) == "Aperçus : PDFium", status_bar(page))
        status = page.js("window.__TAURI__.core.invoke('renderer_status')", wait=True)
        check("renderer_status: available, PDFium loaded", status["available"] and "PDFium chargé depuis" in status["detail"], json.dumps(status))

        first = one_worker(proc, "after the first pages")
        pids = [first["ProcessId"]]
        line = first.get("CommandLine") or ""
        check("its command line is the executable and the worker argument, nothing else",
              line.rstrip().endswith(ARGUMENT) and "heavy" not in line, line)
        check("the window itself was given the file, not the worker",
              any("heavy.pdf" in (p["CommandLine"] or "") and p["ProcessId"] == proc.pid for p in app_processes()))

        # Killed between two requests: the next page is drawn by a new worker.
        kill(pids[-1])
        check("the worker killed: none left", workers(proc.pid) == [])
        outcome = render(page, 1, 800)
        check("killed between two requests: the next page is drawn all the same", outcome[0] == "ok" and outcome[1] > 0, json.dumps(outcome))
        second = one_worker(proc, "after the kill")
        check("it is another process", second["ProcessId"] != pids[-1])
        pids.append(second["ProcessId"])

        # Killed while it draws: the request fails in words, the next one succeeds.
        page.js(
            "window.__drawn = null; window.__TAURI__.core.invoke('render_page', {page: 0, width: 4096})"
            ".then(() => { window.__drawn = ['ok', '']; }, (e) => { window.__drawn = ['error', (e && e.message) || String(e)]; }); 1"
        )
        time.sleep(0.2)
        check("the page is still being drawn when the worker is killed", page.js("window.__drawn") is None)
        kill(pids[-1])
        drawn = wait_until("the answer to the request", lambda: page.js("window.__drawn"), timeout=60)
        check("killed while it draws: the request fails, and says the engine will be started again",
              drawn == ["error", "le moteur de rendu s'est arrêté ; il sera relancé"], json.dumps(drawn))
        check("no worker until a page is asked for", workers(proc.pid) == [])
        outcome = render(page, 0, 300)
        check("the same page, asked again, is drawn by a new worker", outcome[0] == "ok" and outcome[1] > 0, json.dumps(outcome)[:200])
        third = one_worker(proc, "after the request that failed")
        pids.append(third["ProcessId"])
        check("the status bar still names the renderer", status_bar(page) == "Aperçus : PDFium", status_bar(page))

        # The page that brought the worker down once does it again: refused
        # from then on, without a worker being started for it.
        page.js(
            "window.__drawn = null; window.__TAURI__.core.invoke('render_page', {page: 0, width: 4096})"
            ".then(() => { window.__drawn = ['ok', '']; }, (e) => { window.__drawn = ['error', (e && e.message) || String(e)]; }); 1"
        )
        time.sleep(0.2)
        kill(pids[-1])
        drawn = wait_until("the answer to the request", lambda: page.js("window.__drawn"), timeout=60)
        check("killed again on the same page: the request fails", drawn[0] == "error" and "s'est arrêté" in drawn[1], json.dumps(drawn))
        outcome = render(page, 0, 300)
        check("the page that fell twice is refused, counted from 1",
              outcome == ["error", "la page 1 a arrêté le moteur de rendu deux fois ; elle n'est plus dessinée"], json.dumps(outcome))
        check("no worker was started for it", workers(proc.pid) == [])
        outcome = render(page, 1, 300)
        check("the other page is drawn", outcome[0] == "ok", json.dumps(outcome)[:200])
        pids.append(one_worker(proc, "after the refusal")["ProcessId"])

        # Too many restarts within a minute: rendering is off, and the
        # status bar says so once the interface asks for a page.
        refusal = None
        for _ in range(12):
            mine = workers(proc.pid)
            if mine:
                kill(mine[0]["ProcessId"])
            outcome = render(page, 1, 200)
            if outcome[0] == "error":
                refusal = outcome[1]
                break
            pids.append(workers(proc.pid)[0]["ProcessId"])
        restarts = len(set(pids)) - 1
        check("eight restarts within a minute, then rendering is refused",
              restarts == 8 and refusal is not None and "trop souvent (8 relances" in refusal, f"{restarts} restarts, {refusal}")
        check("no worker is started any more", workers(proc.pid) == [])
        status = page.js("window.__TAURI__.core.invoke('renderer_status')", wait=True)
        check("renderer_status says rendering is off, and why", not status["available"] and status["detail"] == refusal, json.dumps(status))
        # The page view asks for its page: the failure reaches the interface.
        page.click('#grid .tile[data-position="1"]')
        page.enter()
        bar = wait_until("the status bar", lambda: status_bar(page).startswith("Aperçus indisponibles") and status_bar(page))
        check("the status bar says rendering is off, and why", "trop souvent" in bar and "redémarrage de 4YouPDF" in bar, bar)
        shown = wait_until("the page view", lambda: page.js("document.querySelector('#viewer-page').textContent"))
        check("the page view says why its page is not drawn", shown.startswith("Rendu impossible : ") and "trop souvent" in shown, shown)
        check("the window still answers", page.js("document.querySelectorAll('#grid .tile').length") == 2)
    finally:
        base.stop(proc)
    time.sleep(1)
    check("the window stopped: no fyp-app process left", app_processes() == [], json.dumps(app_processes()))

    # -- Closing the window stops the worker --------------------------------
    proc, page = base.launch(FIXTURE)
    try:
        wait_until("the thumbnails", lambda: tiles(page)["loaded"] >= 1, timeout=40)
        worker = one_worker(proc, "second run")
        hwnd = wait_until("the window", lambda: main_window(proc.pid))
        user32.PostMessageW(hwnd, WM_CLOSE, 0, 0)
        try:
            code = proc.wait(timeout=20)
        except subprocess.TimeoutExpired:
            code = None
        check("WM_CLOSE on an unmodified document: the window process ends with status 0", code == 0, str(code))
        left = app_processes()
        check("the worker ended before the window did", not any(p["ProcessId"] == worker["ProcessId"] for p in left), json.dumps(left))
        check("no fyp-app process left", left == [], json.dumps(left))
    finally:
        base.stop(proc)

    # -- The window killed, as a crash: the worker does not outlive it ------
    proc, page = base.launch(FIXTURE)
    try:
        wait_until("the thumbnails", lambda: tiles(page)["loaded"] >= 1, timeout=40)
        worker = one_worker(proc, "third run")
        kernel32.TerminateProcess(int(proc._handle), 1)
        proc.wait(timeout=20)
        gone = True
        try:
            wait_until("the end of the worker", lambda: not any(p["ProcessId"] == worker["ProcessId"] for p in app_processes()), timeout=10)
        except RuntimeError:
            gone = False
        check("the window killed: its worker does not outlive it", gone, json.dumps(app_processes()))
    finally:
        base.stop(proc)

    # -- The window killed while its worker draws: the job ends the worker ---
    proc, page = base.launch(endless_pdf(os.path.join(tmp, "endless.pdf")))
    try:
        worker = wait_until("the worker", lambda: (workers(proc.pid) or [None])[0], timeout=40)
        wait_until("the tile of the page", lambda: tiles(page)["count"] == 1, timeout=40)
        time.sleep(2)
        seen = tiles(page)
        check("the thumbnail of the endless page is still being drawn", seen["loaded"] == 0 and not seen["failed"], json.dumps(seen))
        check("by the same worker", [p["ProcessId"] for p in workers(proc.pid)] == [worker["ProcessId"]])
        kernel32.TerminateProcess(int(proc._handle), 1)
        proc.wait(timeout=20)
        killed = time.time()
        gone = True
        try:
            wait_until("the end of the worker", lambda: not any(p["ProcessId"] == worker["ProcessId"] for p in app_processes()), timeout=3)
        except RuntimeError:
            gone = False
        check("the window killed while its worker draws: the worker is ended at once",
              gone, f"{time.time() - killed:.1f} s, {json.dumps(app_processes())}")
    finally:
        base.stop(proc)
        for left in app_processes():
            kill(left["ProcessId"])
        shutil.rmtree(tmp, ignore_errors=True)

    failed = [name for name, ok in base.RESULTS if not ok]
    print(f"\n{len(base.RESULTS) - len(failed)}/{len(base.RESULTS)} checks passed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
