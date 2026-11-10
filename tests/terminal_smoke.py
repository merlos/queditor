"""Unix PTY smoke tests. Run after `cargo build`: python3 tests/terminal_smoke.py."""

import fcntl
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import tempfile
import termios
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target" / "debug" / "queditor"


class TerminalApp:
    def __init__(self, document, config, width=140, height=40):
        self.master, self.slave = pty.openpty()
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
        self.original_attrs = termios.tcgetattr(self.slave)
        env = dict(os.environ, TERM="xterm-256color", XDG_CONFIG_HOME=str(config))

        def attach_terminal():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        self.process = subprocess.Popen(
            [str(BINARY), str(document)], stdin=self.slave, stdout=self.slave,
            stderr=self.slave, env=env, cwd=ROOT, preexec_fn=attach_terminal,
        )
        self.output = bytearray()
        self.wait_for(b"MOCK")

    def wait_for(self, needle, timeout=5):
        deadline = time.monotonic() + timeout
        start = len(self.output)
        while time.monotonic() < deadline:
            if needle in self.output[start:]:
                return
            ready, _, _ = select.select([self.master], [], [], 0.1)
            if ready:
                self.output.extend(os.read(self.master, 65536))
            if self.process.poll() is not None:
                break
        raise AssertionError(f"Terminal did not render {needle!r}: {bytes(self.output[-2000:])!r}")

    def send(self, keys, rendered=None):
        os.write(self.master, keys)
        if rendered:
            self.wait_for(rendered)

    def finish(self):
        self.process.wait(timeout=5)
        if self.process.returncode != 0:
            raise AssertionError(f"Editor exited with {self.process.returncode}")
        if termios.tcgetattr(self.slave) != self.original_attrs:
            raise AssertionError("Terminal attributes were not restored")

    def close(self):
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait()
        os.close(self.master)
        os.close(self.slave)


class SmokeTests(unittest.TestCase):
    def launch(self, directory, width=140):
        doc = Path(directory) / "draft.md"
        doc.write_text("", encoding="utf-8")
        app = TerminalApp(doc, Path(directory) / "config", width=width)
        self.addCleanup(app.close)
        return doc, app

    def test_type_save_exit_restores_terminal(self):
        with tempfile.TemporaryDirectory(prefix="queditor-smoke-") as directory:
            doc, app = self.launch(directory)
            app.send(b"hello\rworld\x13", b"Saved")
            app.send(b"\x11")
            app.finish()
            self.assertEqual(doc.read_text(), "hello\nworld")

    def test_mock_review_accept_undo_and_save(self):
        with tempfile.TemporaryDirectory(prefix="queditor-smoke-") as directory:
            doc, app = self.launch(directory, width=80)
            app.send(b"hello\t/demo\r", b"MOCK PROPOSAL")
            app.send(b"a", b"Proposal accepted")
            app.send(b"\x13", b"Saved")
            self.assertEqual(doc.read_text(), "HELLO")
            app.send(b"\t\x1a", b"hello")
            app.send(b"\x13\x11")
            app.finish()
            self.assertEqual(doc.read_text(), "hello")

    def test_dirty_quit_cancel_and_discard(self):
        with tempfile.TemporaryDirectory(prefix="queditor-smoke-") as directory:
            doc, app = self.launch(directory)
            app.send(b"unsaved\x11", b"Confirm")
            app.send(b"c")
            app.send(b"\x11", b"Confirm")
            app.send(b"d")
            app.finish()
            self.assertEqual(doc.read_text(), "")


if __name__ == "__main__":
    unittest.main()
