#!/usr/bin/env python3
"""Smoke test of the window, driven through the DevTools port of WebView2
(CDP): choosing the pages of each file merged, and an extraction that never
replaces the file open (rampe v0.5.0, session C); then, since its
correctifs, how long a refusal stays in the banner of the merge.

What runs for real: the interface, the Rust commands (`pick_merge_files`
once through the native picker, `merge_documents`, `save_document`) and the
files written. What is stubbed: the native pickers of the other scenarios,
answered by a wrapper of `window.__TAURI__.core.invoke`; the page count of
each file then comes from this script (`candidate`), the Rust side counting
them for real in the one scenario with the native picker. Synthetic input
and DOM state only: nothing here looks at the screen.

Needs a development build that embeds the current interface:

    python tools/build_ui.py
    cargo build -p fyp-app
    python tools/ui_smoke/merge_pages.py [path/to/fyp-app.exe]

No other 4YouPDF may run: instances share the WebView2 profile. Windows
only (native picker driven by window messages). Exit status 1 when a check
fails.
"""

import base64
import ctypes
import ctypes.wintypes as wt
import hashlib
import json
import os
import re
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import time
import urllib.request
import zlib

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
EXE = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "target", "debug", "fyp-app.exe")
FIXTURES = os.path.join(ROOT, "tests", "fixtures")
PORT = 9333
RESULTS = []


def check(name, ok, detail=""):
    RESULTS.append((name, bool(ok)))
    print(("    ok      " if ok else "    FAILED  ") + name + (f"  [{detail}]" if detail and not ok else ""))


# ---------------------------------------------------------------------------
# A minimal CDP client over a stdlib WebSocket
# ---------------------------------------------------------------------------


class WebSocket:
    def __init__(self, url):
        rest = url[len("ws://"):]
        hostport, _, path = rest.partition("/")
        host, _, port = hostport.partition(":")
        self.sock = socket.create_connection((host, int(port)), timeout=30)
        key = base64.b64encode(os.urandom(16)).decode()
        self.sock.sendall(
            (
                f"GET /{path} HTTP/1.1\r\nHost: {hostport}\r\nUpgrade: websocket\r\n"
                f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
            ).encode()
        )
        response = b""
        while b"\r\n\r\n" not in response:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise RuntimeError("WebSocket handshake failed")
            response += chunk
        if b" 101 " not in response.split(b"\r\n")[0]:
            raise RuntimeError("WebSocket handshake refused")
        self.buf = response.split(b"\r\n\r\n", 1)[1]

    def _exact(self, n):
        while len(self.buf) < n:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise RuntimeError("socket closed")
            self.buf += chunk
        data, self.buf = self.buf[:n], self.buf[n:]
        return data

    def send(self, text):
        payload = text.encode()
        header = bytearray([0x81])
        n = len(payload)
        if n < 126:
            header.append(0x80 | n)
        elif n < 65536:
            header.append(0x80 | 126)
            header += struct.pack(">H", n)
        else:
            header.append(0x80 | 127)
            header += struct.pack(">Q", n)
        mask = os.urandom(4)
        header += mask
        self.sock.sendall(bytes(header) + bytes(b ^ mask[i % 4] for i, b in enumerate(payload)))

    def recv(self):
        message = b""
        while True:
            b0, b1 = self._exact(2)
            n = b1 & 0x7F
            if n == 126:
                n = struct.unpack(">H", self._exact(2))[0]
            elif n == 127:
                n = struct.unpack(">Q", self._exact(8))[0]
            data = self._exact(n)
            opcode = b0 & 0x0F
            if opcode == 0x9:
                self.sock.sendall(bytes(bytearray([0x8A, 0x80]) + os.urandom(4)))
                continue
            if opcode == 0x8:
                raise RuntimeError("closed by peer")
            message += data
            if b0 & 0x80:
                return message.decode()


class Page:
    def __init__(self, timeout=60):
        deadline = time.time() + timeout
        while True:
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/list", timeout=2) as r:
                    pages = [t for t in json.load(r) if t.get("type") == "page" and "tauri.localhost" in t.get("url", "")]
                if pages:
                    break
            except OSError:
                pass
            if time.time() > deadline:
                raise RuntimeError(f"no page on port {PORT}")
            time.sleep(0.25)
        self.ws = WebSocket(pages[0]["webSocketDebuggerUrl"])
        self.next_id = 1

    def call(self, method, **params):
        id_ = self.next_id
        self.next_id += 1
        self.ws.send(json.dumps({"id": id_, "method": method, "params": params}))
        while True:
            msg = json.loads(self.ws.recv())
            if msg.get("id") == id_:
                if "error" in msg:
                    raise RuntimeError(f"{method}: {msg['error']}")
                return msg.get("result", {})

    def js(self, expression, wait=False):
        r = self.call("Runtime.evaluate", expression=expression, returnByValue=True, awaitPromise=wait)
        if "exceptionDetails" in r:
            raise RuntimeError(json.dumps(r["exceptionDetails"])[:400])
        return r.get("result", {}).get("value")

    def key(self, vk, code, key, modifiers=0, text=None):
        params = dict(windowsVirtualKeyCode=vk, nativeVirtualKeyCode=vk, code=code, key=key, modifiers=modifiers)
        if text is not None:
            self.call("Input.dispatchKeyEvent", type="keyDown", text=text, unmodifiedText=text, **params)
        else:
            self.call("Input.dispatchKeyEvent", type="rawKeyDown", **params)
        self.call("Input.dispatchKeyEvent", type="keyUp", **params)

    def ctrl(self, letter):
        self.key(ord(letter.upper()), f"Key{letter.upper()}", letter.lower(), modifiers=2)

    def enter(self):
        self.key(13, "Enter", "Enter", text="\r")

    def click(self, selector, button="left"):
        rect = self.js(
            f"(() => {{ const e = document.querySelector({json.dumps(selector)}); if (!e) return null; "
            "e.scrollIntoView({block: 'nearest'}); const r = e.getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()"
        )
        if rect is None:
            raise RuntimeError(f"no element {selector}")
        x, y = rect[0] + rect[2] / 2, rect[1] + rect[3] / 2
        self.call("Input.dispatchMouseEvent", type="mouseMoved", x=x, y=y)
        self.call("Input.dispatchMouseEvent", type="mousePressed", x=x, y=y, button=button, clickCount=1)
        self.call("Input.dispatchMouseEvent", type="mouseReleased", x=x, y=y, button=button, clickCount=1)

    def type_in(self, selector, text):
        """Replace what the field holds with `text`, as typed."""
        self.js(f"(() => {{ const e = document.querySelector({json.dumps(selector)}); e.focus(); e.select(); }})()")
        if text == "":
            self.key(46, "Delete", "Delete")
        else:
            self.call("Input.insertText", text=text)


# ---------------------------------------------------------------------------
# The application
# ---------------------------------------------------------------------------


def port_open():
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/version", timeout=1):
            return True
    except OSError:
        return False


# Every command goes through; the pickers answer what the script set, unless
# `window.__native` is set, when the real picker opens.
WRAP = r"""
(() => {
  const real = window.__TAURI__;
  const invoke = real.core.invoke.bind(real.core);
  window.__calls = [];
  window.__picked = null;
  window.__saveTo = null;
  window.__native = false;
  window.__TAURI__ = {
    ...real,
    core: {
      ...real.core,
      invoke: (command, args) => {
        window.__calls.push([command, args ?? null]);
        if (command === "pick_merge_files" && !window.__native) {
          return Promise.resolve(window.__picked);
        }
        if (command === "pick_save_file") {
          return Promise.resolve(window.__saveTo);
        }
        return invoke(command, args);
      },
    },
  };
  return true;
})()
"""

STATE = r"""
(() => {
  const form = document.querySelector("#notices form.merge");
  return {
    tiles: [...document.querySelectorAll("#grid .tile")].map((t) => Number(t.dataset.page)),
    selected: [...document.querySelectorAll("#grid .tile.selected")].map((t) => Number(t.dataset.position)),
    modified: document.getElementById("doc-name").classList.contains("modified"),
    notices: [...document.querySelectorAll("#notices .notice")].map((n) => [n.className.replace("notice ", ""), n.querySelector("span").textContent]),
    status: document.getElementById("status-text").textContent,
    split: document.querySelector("#notices form.split") !== null,
    merge: form === null ? null : {
      rows: [...form.querySelectorAll(".merge-name")].map((n) => n.textContent),
      fields: [...form.querySelectorAll("input.merge-pages")].map((f) => f.value),
      notes: [...form.querySelectorAll(".merge-note")].map((n) => [n.textContent, n.classList.contains("refused")]),
      preview: form.querySelector(".merge-preview").textContent,
      error: form.querySelector(".merge-error").hidden ? "" : form.querySelector(".merge-error").textContent,
      focused: document.activeElement instanceof HTMLInputElement && form.contains(document.activeElement)
        ? Number(document.activeElement.dataset.row) : null,
    },
    calls: window.__calls.map((c) => c[0]),
    merges: window.__calls.filter((c) => c[0] === "merge_documents").map((c) => c[1]),
  };
})()
"""


def state(page):
    return page.js(STATE)


def wait(page, what, predicate, timeout=20.0):
    end = time.time() + timeout
    last = None
    while time.time() < end:
        last = state(page)
        if predicate(last):
            return last
        time.sleep(0.1)
    raise RuntimeError(f"waiting for {what}: {json.dumps(last, ensure_ascii=False)[:600]}")


def launch(pdf):
    if port_open():
        raise RuntimeError(f"port {PORT} is open: another 4YouPDF runs")
    env = dict(os.environ, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=f"--remote-debugging-port={PORT}")
    proc = subprocess.Popen([EXE, pdf], env=env, cwd=ROOT)
    page = Page()
    page.js(
        "new Promise((r) => { const t = () => location.href.startsWith('http://tauri.localhost') "
        "&& window.__TAURI__ ? r(1) : setTimeout(t, 100); t(); })",
        wait=True,
    )
    page.js(WRAP)
    return proc, page


def stop(proc):
    subprocess.run(
        ["powershell", "-NoProfile", "-Command", f"Stop-Process -Id {proc.pid} -Force -ErrorAction SilentlyContinue"],
        capture_output=True,
    )
    proc.wait(timeout=20)
    end = time.time() + 20
    while time.time() < end and port_open():
        time.sleep(0.2)


# ---------------------------------------------------------------------------
# Files: pages that say what they are, and reading them back
# ---------------------------------------------------------------------------


def labelled(path, prefix, count):
    """`count` A4 pages, each showing `<prefix><n>`."""
    kids = " ".join(f"{3 + 2 * i} 0 R" for i in range(count))
    objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        f"<< /Type /Pages /Kids [{kids}] /Count {count} /MediaBox [0 0 595 842] "
        "/Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>",
    ]
    for i in range(count):
        text = f"BT /F1 48 Tf 72 700 Td ({prefix}{i + 1}) Tj ET"
        objects.append(f"<< /Type /Page /Parent 2 0 R /Contents {4 + 2 * i} 0 R >>")
        objects.append(f"<< /Length {len(text)} >>\nstream\n{text}\nendstream")
    out = b"%PDF-1.7\n"
    offsets = []
    for i, body in enumerate(objects):
        offsets.append(len(out))
        out += f"{i + 1} 0 obj\n{body}\nendobj\n".encode()
    start = len(out)
    out += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode()
    for offset in offsets:
        out += f"{offset:010} 00000 n \n".encode()
    out += f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n".encode()
    with open(path, "wb") as f:
        f.write(out)
    return path


def read_pages(path):
    """The label and the /Rotate of each page of a file written by the
    application: a single section, objects in the clear, streams in the
    clear or deflated."""
    data = open(path, "rb").read()
    objects = {}
    for m in re.finditer(rb"(\d+) 0 obj(.*?)endobj", data, re.S):
        objects[int(m.group(1))] = m.group(2)

    def stream(num):
        body = objects[num]
        raw = re.search(rb"stream\r?\n(.*)\r?\nendstream", body, re.S).group(1)
        return zlib.decompress(raw) if b"FlateDecode" in body else raw

    catalog = next(b for b in objects.values() if re.search(rb"/Type\s*/Catalog", b))
    root = int(re.search(rb"/Pages\s+(\d+) 0 R", catalog).group(1))
    kids = [int(k) for k in re.findall(rb"(\d+) 0 R", re.search(rb"/Kids\s*\[(.*?)\]", objects[root], re.S).group(1))]
    pages = []
    for kid in kids:
        body = objects[kid]
        rotate = re.search(rb"/Rotate\s+(\d+)", body)
        contents = int(re.search(rb"/Contents\s+(\d+) 0 R", body).group(1))
        label = re.search(rb"\((\w+)\)", stream(contents)).group(1).decode()
        pages.append((label, int(rotate.group(1)) if rotate else 0))
    return pages


def candidate(path, pages=None, status=None):
    """A file as `pick_merge_files` describes it once chosen."""
    return {
        "path": path,
        "name": os.path.basename(path),
        "status": status or {"kind": "ready", "pages": pages},
    }


def digest(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


# ---------------------------------------------------------------------------
# The native picker, driven by window messages
# ---------------------------------------------------------------------------

user32 = ctypes.windll.user32
WNDENUMPROC = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
user32.SendMessageW.argtypes = [wt.HWND, wt.UINT, wt.WPARAM, wt.LPARAM]
user32.SendMessageW.restype = ctypes.c_ssize_t
user32.FindWindowExW.argtypes = [wt.HWND, wt.HWND, wt.LPCWSTR, wt.LPCWSTR]
user32.FindWindowExW.restype = wt.HWND
user32.GetDlgItem.argtypes = [wt.HWND, ctypes.c_int]
user32.GetDlgItem.restype = wt.HWND
WM_SETTEXT, WM_CLOSE, BM_CLICK = 0x000C, 0x0010, 0x00F5


def picker(pid, title, timeout=20):
    """The visible dialog of process `pid` titled `title`."""
    end = time.time() + timeout
    while time.time() < end:
        found = []

        def each(hwnd, _):
            owner = wt.DWORD()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
            if owner.value == pid and user32.IsWindowVisible(hwnd):
                name = ctypes.create_unicode_buffer(64)
                user32.GetClassNameW(hwnd, name, 64)
                text = ctypes.create_unicode_buffer(256)
                user32.GetWindowTextW(hwnd, text, 256)
                if name.value == "#32770" and text.value == title:
                    found.append(hwnd)
            return True

        user32.EnumWindows(WNDENUMPROC(each), 0)
        if found:
            return found[0]
        time.sleep(0.2)
    return None


def file_name_box(dialog):
    combo = user32.FindWindowExW(dialog, None, "ComboBoxEx32", None)
    if not combo:
        # Some layouts nest it one level down.
        child = user32.FindWindowExW(dialog, None, "DUIViewWndClassName", None)
        combo = user32.FindWindowExW(child, None, "ComboBoxEx32", None) if child else None
    if not combo:
        return None
    box = user32.FindWindowExW(combo, None, "ComboBox", None)
    return user32.FindWindowExW(box, None, "Edit", None) if box else None


def choose(dialog, text):
    """Type `text` in the file name box of `dialog`, then press Ouvrir."""
    edit = file_name_box(dialog)
    if not edit:
        return False
    # The buffer must outlive the call: the dialog reads it then.
    buffer = ctypes.create_unicode_buffer(text)
    user32.SendMessageW(edit, WM_SETTEXT, 0, ctypes.addressof(buffer))
    user32.SendMessageW(user32.GetDlgItem(dialog, 1), BM_CLICK, 0, 0)
    return True


# ---------------------------------------------------------------------------
# Scenarios
# ---------------------------------------------------------------------------


def ask_merge(page, candidates, context_menu_at=None):
    page.js(f"window.__picked = {json.dumps(candidates)}")
    # The keys of the window do nothing while a field has the keyboard.
    page.js("document.activeElement?.blur()")
    if context_menu_at is None:
        page.ctrl("m")
    else:
        page.click(f'#grid .tile[data-position="{context_menu_at}"]', button="right")
        page.click('#context-menu button[data-action="merge-here"]')
    return wait(page, "the banner of the merge", lambda s: s["merge"] is not None and len(s["merge"]["rows"]) == len(candidates))


def main():
    if not os.path.exists(EXE):
        sys.exit(f"{EXE} missing: python tools/build_ui.py, then cargo build -p fyp-app")
    tmp = tempfile.mkdtemp(prefix="fyp-merge-pages-")
    a = labelled(os.path.join(tmp, "A.pdf"), "A", 4)
    b = labelled(os.path.join(tmp, "B.pdf"), "B", 5)
    c = labelled(os.path.join(tmp, "C.pdf"), "C", 4)
    protected = os.path.join(FIXTURES, "encrypted-user-password.pdf")
    proc, page = launch(a)
    try:
        s = wait(page, "A.pdf open", lambda s: s["tiles"] == [0, 1, 2, 3], timeout=40)
        check("A.pdf open, four pages, not modified", not s["modified"])

        # -- Préalable: an extraction never replaces the file open ----------
        before = digest(a)
        page.click('#grid .tile[data-position="1"]')
        page.js(f"window.__saveTo = {json.dumps(os.path.join(tmp, 'a.PDF'))}")
        page.ctrl("e")
        s = wait(page, "the extraction refused", lambda s: any("Extraction impossible" in n[1] for n in s["notices"]))
        refusal = next(n for n in s["notices"] if "Extraction impossible" in n[1])
        check(
            "extracting over the file open, other capitals: refused, in a notice that names it",
            refusal[0] == "error" and "« A.pdf » est le fichier du document ouvert" in refusal[1] and "Rien n'a été écrit." in refusal[1],
            refusal[1],
        )
        check("the file open keeps its bytes", digest(a) == before)
        check("the document is still not modified", not state(page)["modified"])
        other = labelled(os.path.join(tmp, "autre.pdf"), "Z", 1)
        page.js(f"window.__saveTo = {json.dumps(other)}")
        page.ctrl("e")
        wait(page, "the extraction written", lambda s: s["status"].startswith("Extrait :"))
        check("extracting over another file that exists replaces it", read_pages(other) == [("A2", 0)], str(read_pages(other)))

        # -- The document open as the grid shows it: turned, one page gone --
        page.click('#grid .tile[data-position="0"]')
        page.key(82, "KeyR", "r", text="r")
        wait(page, "page 1 turned", lambda s: "pivotée" in s["status"])
        page.click('#grid .tile[data-position="1"]')
        page.key(46, "Delete", "Delete")
        s = wait(page, "page 2 deleted", lambda s: s["tiles"] == [0, 2, 3])
        check("the grid: A1 turned, A2 deleted", s["modified"])

        # -- The banner --------------------------------------------------------
        s = ask_merge(page, [candidate(b, 5), candidate(c, 4), candidate(protected, status={"kind": "protected"})])
        m = s["merge"]
        check("Ctrl+M: a banner, one row per file chosen", m["rows"] == ["« B.pdf »", "« C.pdf »", "« encrypted-user-password.pdf »"], str(m["rows"]))
        check("a field per file that opens, none for the protected one", m["fields"] == ["", ""], str(m["fields"]))
        check(
            "each row says what it takes, the protected file why it is skipped",
            [n[0] for n in m["notes"]] == ["toutes", "toutes", "protégé par un mot de passe, il sera ignoré"],
            str(m["notes"]),
        )
        check(
            "empty fields: every page, previewed with the total",
            m["preview"] == "9 pages ajoutées à la fin : 5 de « B.pdf », 4 de « C.pdf » ; le document en aura 12.",
            m["preview"],
        )
        check("the keyboard is in the first field", m["focused"] == 0, str(m["focused"]))

        page.type_in('input.merge-pages[data-row="0"]', "5-3")
        s = wait(page, "B reversed", lambda s: s["merge"]["notes"][0][0] == "3 pages")
        check(
            "« 5-3 » for B: three pages, the preview follows",
            s["merge"]["preview"] == "7 pages ajoutées à la fin : 3 de « B.pdf », 4 de « C.pdf » ; le document en aura 10.",
            s["merge"]["preview"],
        )

        merges_before = len(s["merges"])
        for typed, message in [
            ("0", "page 0 hors limites : le document a 4 pages, numérotées de 1 à 4"),
            ("abc", "numéro de page invalide : « abc »"),
            ("5", "page 5 hors limites : le document a 4 pages, numérotées de 1 à 4"),
            ("4,4", "la page 4 est demandée deux fois"),
        ]:
            page.type_in('input.merge-pages[data-row="1"]', typed)
            s = wait(page, f"{typed} refused", lambda s: s["merge"]["notes"][1] == [message, True])
            check(
                f"« {typed} » for C: refused beside its field",
                s["merge"]["preview"] == "Rien n'est fusionné tant qu'une liste de pages est refusée.",
                s["merge"]["preview"],
            )
            page.enter()
            time.sleep(0.4)
            s = state(page)
            check(
                f"« {typed} », Entrée: nothing merged, the banner stays, the keyboard on the field refused",
                len(s["merges"]) == merges_before and s["merge"] is not None and s["merge"]["focused"] == 1 and s["tiles"] == [0, 2, 3],
                json.dumps(s["merge"], ensure_ascii=False)[:200],
            )

        page.type_in('input.merge-pages[data-row="1"]', "1,4")
        wait(page, "C two pages", lambda s: s["merge"]["notes"][1] == ["2 pages", False])
        page.enter()
        s = wait(page, "the merge done", lambda s: s["merge"] is None and len(s["tiles"]) == 8)
        sent = s["merges"][-1] if s["merges"] else None
        check(
            "Entrée: merge_documents asked for these pages, 0-based, null for the file skipped",
            sent is not None and sent["pages"] == [[4, 3, 2], [0, 3], None],
            json.dumps(sent),
        )
        check("the banner closes; the pages come after those of the grid", s["tiles"] == [0, 2, 3, 4, 5, 6, 7, 8], str(s["tiles"]))
        check(
            "the status counts the pages and the file skipped",
            s["status"] == "5 pages de 2 fichiers ajoutées ; 1 fichier ignoré (Ctrl+Z pour annuler).",
            s["status"],
        )
        check("the protected file gets its notice, as before", any("est protégé par un mot de passe" in n[1] for n in s["notices"]))

        saved = os.path.join(tmp, "fusion.pdf")
        page.js(f"window.__saveTo = {json.dumps(saved)}")
        page.ctrl("s")
        s = wait(page, "saved", lambda s: s["status"].startswith("Enregistré :"))
        got = read_pages(saved)
        check(
            "the file saved holds exactly these pages, in this order, A1 turned",
            got == [("A1", 90), ("A3", 0), ("A4", 0), ("B5", 0), ("B4", 0), ("B3", 0), ("C1", 0), ("C4", 0)],
            str(got),
        )

        # -- Nothing typed: the whole file, as before; Ctrl+Z takes it out ---
        ask_merge(page, [candidate(c, 4)])
        page.enter()
        s = wait(page, "all of C merged", lambda s: s["merge"] is None and len(s["tiles"]) == 12)
        check("nothing typed: every page of the file, null sent", s["merges"][-1]["pages"] == [None], json.dumps(s["merges"][-1]))
        page.ctrl("z")
        s = wait(page, "undone", lambda s: len(s["tiles"]) == 8)
        check("Ctrl+Z takes the pages merged out, as before", s["status"] == "Annulé.", s["status"])

        # -- Fusionner ici…: in front of a page, followed by the preview -----
        s = ask_merge(page, [candidate(b, 5)], context_menu_at=1)
        check(
            "« Fusionner ici… » on the second tile: in front of page 2",
            s["merge"]["preview"] == "5 pages de « B.pdf » ajoutées devant la page 2 ; le document en aura 13.",
            s["merge"]["preview"],
        )
        page.click('#grid .tile[data-position="0"]')
        page.key(46, "Delete", "Delete")
        s = wait(page, "page 1 deleted", lambda s: len(s["tiles"]) == 7)
        check(
            "a page deleted before it: the preview follows the page, now the first",
            s["merge"]["preview"] == "5 pages de « B.pdf » ajoutées devant la page 1 ; le document en aura 12.",
            s["merge"]["preview"],
        )
        page.click('#grid .tile[data-position="0"]')
        page.key(46, "Delete", "Delete")
        s = wait(page, "the page meant deleted", lambda s: len(s["tiles"]) == 6)
        check(
            "the page meant deleted: the preview says so",
            s["merge"]["preview"] == "La page devant laquelle fusionner n'est plus dans le document.",
            s["merge"]["preview"],
        )
        page.click("#notices form.merge button.primary")
        time.sleep(0.4)
        s = state(page)
        check(
            "asked anyway: refused in the banner, nothing merged",
            s["merge"] is not None and s["merge"]["error"].startswith("La page devant laquelle fusionner a été supprimée") and len(s["tiles"]) == 6,
            s["merge"]["error"] if s["merge"] else "",
        )
        page.ctrl("z")
        s = wait(page, "the page meant back", lambda s: len(s["tiles"]) == 7)
        check(
            "Ctrl+Z brings the page meant back: the refusal goes, the preview follows the page",
            s["merge"] is not None
            and s["merge"]["error"] == ""
            and s["merge"]["preview"] == "5 pages de « B.pdf » ajoutées devant la page 1 ; le document en aura 12.",
            json.dumps(s["merge"], ensure_ascii=False)[:300],
        )

        # -- A refusal of the Rust side stays until a field changes ----------
        page.key(27, "Escape", "Escape")
        wait(page, "the banner closed", lambda s: s["merge"] is None)
        # B has five pages; said to have nine, the banner takes page 9, and
        # the Rust side, which reads the file again, refuses it.
        ask_merge(page, [candidate(b, 9)])
        page.type_in('input.merge-pages[data-row="0"]', "9")
        wait(page, "page 9 taken by the banner", lambda s: s["merge"]["notes"][0] == ["1 page", False])
        merges_before = len(state(page)["merges"])
        page.enter()
        s = wait(page, "the Rust side refused", lambda s: s["merge"] is not None and s["merge"]["error"] != "")
        check(
            "a page the file no longer has: refused by the Rust side, in the banner, which stays; nothing merged",
            s["merge"]["error"] == "Fusion impossible : « B.pdf » : page 9 hors limites, le fichier a 5 pages ; rien n'a été fusionné"
            and len(s["merges"]) == merges_before + 1
            and len(s["tiles"]) == 7,
            s["merge"]["error"],
        )
        # A tile not selected yet, so that the click does change the selection.
        target = 3 if s["selected"] == [2] else 2
        page.click(f'#grid .tile[data-position="{target}"]')
        s = wait(page, "a tile selected", lambda s: s["selected"] == [target])
        check(
            "a click on a tile: the refusal of the Rust side stays",
            s["merge"] is not None and s["merge"]["error"].startswith("Fusion impossible : « B.pdf » : page 9 hors limites"),
            json.dumps(s["merge"], ensure_ascii=False)[:300],
        )
        page.type_in('input.merge-pages[data-row="0"]', "1")
        s = wait(page, "page 1 typed", lambda s: s["merge"]["notes"][0] == ["1 page", False] and s["merge"]["fields"] == ["1"])
        check("a field changed: the refusal goes", s["merge"]["error"] == "", s["merge"]["error"])

        # -- One banner at a time; Échap and the page view close it ----------
        page.js("document.activeElement?.blur()")
        page.ctrl("d")
        s = wait(page, "the cut", lambda s: s["split"])
        check("Ctrl+D: the banner of the cut takes the place of the merge", s["merge"] is None)
        ask_merge(page, [candidate(b, 5)])
        check("Ctrl+M: the merge takes the place of the cut", not state(page)["split"])
        page.key(27, "Escape", "Escape")
        s = wait(page, "closed", lambda s: s["merge"] is None)
        check("Échap closes it, nothing merged", len(s["tiles"]) == 7)
        ask_merge(page, [candidate(b, 5)])
        page.js("document.activeElement?.blur()")
        page.click('#grid .tile[data-position="0"]')
        page.enter()
        s = wait(page, "the page view", lambda s: page.js("!document.getElementById('viewer').hidden"))
        check("opening the page view closes it", s["merge"] is None)
        page.key(27, "Escape", "Escape")
        wait(page, "the page view closed", lambda s: page.js("document.getElementById('viewer').hidden"))

        # -- No file to take pages from: refused in the banner ---------------
        s = ask_merge(page, [candidate(protected, status={"kind": "protected"})])
        merges_before = len(s["merges"])
        page.click("#notices form.merge button.primary")
        s = wait(page, "nothing to add", lambda s: s["merge"] is not None and s["merge"]["error"] != "")
        check(
            "no file can give a page: refused in the banner, nothing asked of the Rust side",
            s["merge"]["error"] == "Aucun des fichiers choisis ne peut être fusionné : il n'y a rien à ajouter."
            and len(s["merges"]) == merges_before,
            s["merge"]["error"],
        )
        page.key(27, "Escape", "Escape")
        wait(page, "the banner closed", lambda s: s["merge"] is None)

        # -- The native picker, and the Rust side counting the pages ---------
        page.js("window.__native = true; window.__nativeAnswer = undefined; "
                "window.__TAURI__.core.invoke('pick_merge_files').then((r) => { window.__nativeAnswer = r; }, "
                "(e) => { window.__nativeAnswer = {error: String(e)}; })")
        dialog = picker(proc.pid, "Fusionner à la suite du document")
        typed = dialog is not None and choose(dialog, tmp)
        if typed:
            time.sleep(1.0)
            typed = choose(dialog, '"B.pdf" "C.pdf"')
        answer = None
        end = time.time() + 20
        while time.time() < end:
            answer = page.js("window.__nativeAnswer === undefined ? '…' : window.__nativeAnswer")
            if answer != "…":
                break
            time.sleep(0.2)
        if not typed and dialog:
            user32.PostMessageW(dialog, WM_CLOSE, 0, 0)
        page.js("window.__native = false")
        # The picker gives the files in its own order: not the point here.
        got = sorted(answer, key=lambda f: f["name"]) if isinstance(answer, list) else answer
        check(
            "the native picker: pick_merge_files answers each file with its page count, counted by the Rust side",
            got == [
                {"path": b, "name": "B.pdf", "status": {"kind": "ready", "pages": 5}},
                {"path": c, "name": "C.pdf", "status": {"kind": "ready", "pages": 4}},
            ],
            json.dumps(answer, ensure_ascii=False)[:300],
        )
    finally:
        stop(proc)
        shutil.rmtree(tmp, ignore_errors=True)

    failed = [name for name, ok in RESULTS if not ok]
    print(f"\n{len(RESULTS) - len(failed)}/{len(RESULTS)} checks passed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
