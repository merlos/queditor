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
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import codecs
import re
import unicodedata

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target" / "debug" / "queditor"


class TerminalApp:
    def __init__(self, document, config, width=140, height=40, extra_env=None, backend=b"mock"):
        self.master, self.slave = pty.openpty()
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
        self.original_attrs = termios.tcgetattr(self.slave)
        env = dict(os.environ, TERM="xterm-256color", XDG_CONFIG_HOME=str(config), XDG_STATE_HOME=str(config.parent / "state"))
        env.update(extra_env or {})

        def attach_terminal():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        self.process = subprocess.Popen(
            [str(BINARY)] + ([str(document)] if document is not None else []),
            stdin=self.slave, stdout=self.slave,
            stderr=self.slave, env=env, cwd=ROOT, preexec_fn=attach_terminal,
        )
        self.output = bytearray()
        self.screen = [[" " for _ in range(width)] for _ in range(height)]
        self.screen_row = self.screen_col = 0
        self.screen_pending = ""
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")
        self.wait_for(backend)
        if b"\x1b[>1u" not in self.output:
            raise AssertionError("Editor did not enable enhanced keyboard reporting")

    def feed(self, data):
        """Minimal VT cursor/screen tracking: Ratatui emits diffs, not full text lines."""
        self.output.extend(data)
        self.screen_pending += self.decoder.decode(data)
        text = self.screen_pending
        index = 0
        while index < len(text):
            char = text[index]
            if char == "\x1b":
                if index + 1 >= len(text):
                    break
                if text[index + 1] == "[":
                    match = re.match(r"\x1b\[([0-9;?>]*)([A-Za-z~])", text[index:])
                    if not match:
                        break
                    parameters, command = match.groups()
                    values = [int(p or "0") for p in parameters.lstrip("?>").split(";")]
                    if command in "Hf":
                        self.screen_row = max(0, (values[0] or 1) - 1)
                        self.screen_col = max(0, ((values[1] if len(values) > 1 else 1) or 1) - 1)
                    elif command == "J" and values[0] in (2, 3):
                        self.screen = [[" " for _ in row] for row in self.screen]
                    index += match.end()
                    continue
                index += 2
                continue
            index += 1
            if char == "\r":
                self.screen_col = 0
            elif char == "\n":
                self.screen_row += 1
            elif not char.isprintable():
                continue
            elif unicodedata.combining(char):
                continue
            else:
                if 0 <= self.screen_row < len(self.screen) and 0 <= self.screen_col < len(self.screen[0]):
                    self.screen[self.screen_row][self.screen_col] = char
                self.screen_col += 2 if unicodedata.east_asian_width(char) in ("W", "F") else 1
        self.screen_pending = text[index:]

    def wait_for(self, needle, timeout=5):
        deadline = time.monotonic() + timeout
        start = len(self.output)
        while time.monotonic() < deadline:
            if needle in self.output[start:]:
                return
            visible = " ".join(" ".join("".join(row) for row in self.screen).split())
            if len(self.output) > start and needle.decode() in visible:
                return
            ready, _, _ = select.select([self.master], [], [], 0.1)
            if ready:
                self.feed(os.read(self.master, 65536))
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
    def test_agent_picker_and_prompt_control_line_keys(self):
        with tempfile.TemporaryDirectory(prefix="queditor-picker-prompt-") as directory:
            doc, app = self.launch(directory)
            app.send(b"\t/agents\r", b"Which agent do you want to use?")
            app.send(b"\x1b[B\r", b"Agent selected and remembered: Blogger")
            saved = (Path(directory) / "config/queditor/config.yaml").read_text()
            self.assertIn("agent: Blogger", saved)
            app.send(b"first\x0asecond\x01\x0bkept\x05!\r", b"Mock mode is selected.")
            visible = "\n".join("".join(row) for row in app.screen)
            self.assertIn("first", visible)
            self.assertIn("kept!", visible)
            self.assertNotIn("second", visible)
            app.send(b"\x11")
            app.finish()
            self.assertEqual(doc.read_text(), "")

    def launch(self, directory, width=140):
        doc = Path(directory) / "draft.md"
        doc.write_text("", encoding="utf-8")
        app = TerminalApp(doc, Path(directory) / "config", width=width)
        self.addCleanup(app.close)
        return doc, app

    def test_unnamed_save_popup_from_completion_and_control_s(self):
        for trigger in (b"\t/sa\r", b"\x13"):
            with self.subTest(trigger=trigger), tempfile.TemporaryDirectory(prefix="queditor-save-prompt-") as directory:
                root = Path(directory)
                destination = root / "draft 界 name.md"
                app = TerminalApp(None, root / "config")
                self.addCleanup(app.close)
                app.send(b"hello" + trigger, b"Save document")
                app.send(b"\r", b"Enter a filename")
                self.assertFalse(destination.exists())
                app.send(b"\x1b[200~" + str(destination).encode() + b"\x1b[201~\r", b"Saved")
                self.assertEqual(destination.read_text(), "hello")
                app.send(b"\x11")
                app.finish()

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
            app.send(b"hello\t/demo\r", b"MOCK PREVIEW: Change 1 of 1")
            app.send(b"\x1b[13;5u", b"Proposal accepted")
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

    def test_selected_agent_and_model_are_restored_after_restart(self):
        with tempfile.TemporaryDirectory(prefix="queditor-preferences-") as directory:
            root = Path(directory)
            doc = root / "draft.md"
            doc.write_text("")
            config = root / "config"
            app = TerminalApp(doc, config)
            self.addCleanup(app.close)
            app.send(b"\t\x1b[Z", b"Blogger")
            app.send(b"/mode agent\r", b"AGENT mode selected")
            visible = "\n".join("".join(row) for row in app.screen)
            self.assertNotIn("you:", visible)
            self.assertNotIn("/mode agent", visible)
            self.assertNotIn("draft.md", "".join(app.screen[0]))
            self.assertEqual("".join(app.screen[-2]).strip(), "draft.md · 1:1 · 0 words · 0 chars · 0 b · Tokens in 0 out 0")
            app.send(b"/models ollama::remembered\r", b"Model selected and remembered")
            saved = (config / "queditor/config.yaml").read_text()
            self.assertIn("agent: Blogger", saved)
            self.assertIn("Blogger: agent", saved)
            self.assertIn("model: ollama::remembered", saved)
            app.send(b"\x11")
            app.finish()

            restarted = TerminalApp(doc, config, backend=b"ollama::remembered")
            self.addCleanup(restarted.close)
            restarted.send(b"\t", "Blogger Agent · ollama:remembered".encode())
            self.assertNotIn("Blogger", "".join(restarted.screen[-2]))
            self.assertNotIn("Agent", "".join(restarted.screen[0]))
            self.assertEqual("".join(restarted.screen[-2]).strip(), "draft.md · 1:1 · 0 words · 0 chars · 0 b · Tokens in 0 out 0")
            self.assertNotIn("ollama::remembered", "".join(restarted.screen[-2]))
            visible = "\n".join("".join(row) for row in restarted.screen)
            self.assertIn("Blogger Agent · ollama:remembered", visible)
            restarted.send(b"\x11")
            restarted.finish()


class FakeProvider:
    """Exercises genai's real HTTP/SSE adapter against a local fixture server."""

    def __init__(self, read_path, deny=False, operation="read_file"):
        self.requests = []
        self.read_path = str(read_path)
        self.deny = deny
        self.operation = operation
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                owner.requests.append(body)
                turn = len(owner.requests)
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Connection", "close")
                self.end_headers()

                def chunk(delta, finish=None):
                    data = {"id": "local-test", "object": "chat.completion.chunk", "model": "test", "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
                    self.wfile.write(("data: " + json.dumps(data) + "\n\n").encode())
                    self.wfile.flush()

                if turn == 1 or (turn == 2 and not owner.deny):
                    name = owner.operation if turn == 1 else "propose_edit"
                    arguments = {"url" if owner.operation == "webfetch" else "path": owner.read_path} if turn == 1 else {"replacement": "HELLO"}
                    chunk({"role": "assistant", "tool_calls": [{"index": 0, "id": f"call-{turn}", "type": "function", "function": {"name": name, "arguments": json.dumps(arguments)}}]})
                    chunk({}, "tool_calls")
                else:
                    chunk({"role": "assistant", "content": "Review " if not owner.deny else "Access "})
                    chunk({"content": "ready" if not owner.deny else "denied"})
                    chunk({}, "stop")
                usage = {"id": "local-test", "object": "chat.completion.chunk", "choices": [], "usage": {"prompt_tokens": 100 * turn, "completion_tokens": 10 * turn, "total_tokens": 110 * turn}}
                self.wfile.write(("data: " + json.dumps(usage) + "\n\n").encode())
                self.wfile.write(b"data: [DONE]\n\n")
                self.wfile.flush()

            def do_GET(self):
                if self.path == "/v1/models":
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Connection", "close")
                    self.end_headers()
                    self.wfile.write(json.dumps({"object": "list", "data": [{"id": "test", "object": "model"}]}).encode())
                else:
                    self.send_response(404)
                    self.end_headers()

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.endpoint = f"http://127.0.0.1:{self.server.server_port}/v1/"

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()


class LiveSmokeTests(unittest.TestCase):
    def test_webfetch_requests_permission_to_browse_and_denial_reaches_provider(self):
        with tempfile.TemporaryDirectory(prefix="queditor-web-permission-") as directory:
            root = Path(directory)
            doc = root / "draft.md"
            doc.write_text("unchanged")
            config = root / "config"
            (config / "queditor").mkdir(parents=True)
            (config / "queditor/config.yaml").write_text("model: genai_1::test\n")
            # Reserved .example domain: denied browsing must never resolve/fetch it.
            url = "https://permission-test.example/page?q=rust"
            provider = FakeProvider(url, deny=True, operation="webfetch")
            self.addCleanup(provider.close)
            app = TerminalApp(doc, config, extra_env={"GENAI_1_ENDPOINT": provider.endpoint, "GENAI_1_API_KEY": "local-test-placeholder"}, backend=b"genai_1::test")
            self.addCleanup(app.close)
            app.send(b"\t/mode chat\r", b"CHAT mode selected")
            app.send(b"browse that page\r", b"Allow the agent to browse this URL?")
            self.assertEqual(len(provider.requests), 1)
            visible = "\n".join("".join(row) for row in app.screen)
            self.assertIn(url, visible)
            names = [tool["function"]["name"] for tool in provider.requests[0]["tools"]]
            self.assertIn("webfetch", names)
            self.assertNotIn("propose_edit", names)
            app.send(b"n", b"Agent finished")
            self.assertIn("User denied permission to browse", json.dumps(provider.requests))
            self.assertEqual(doc.read_text(), "unchanged")
            app.send(b"\x11")
            app.finish()

    def test_untitled_agent_uses_temporary_backing_then_save_as_and_review_undo_use_real_file(self):
        with tempfile.TemporaryDirectory(prefix="queditor-untitled-agent-") as directory:
            root = Path(directory)
            note = root / "note.md"
            note.write_text("approved tool context")
            config = root / "config"
            (config / "queditor").mkdir(parents=True)
            (config / "queditor/config.yaml").write_text("model: genai_1::test\n")
            provider = FakeProvider(note)
            self.addCleanup(provider.close)
            app = TerminalApp(None, config, extra_env={"GENAI_1_ENDPOINT": provider.endpoint, "GENAI_1_API_KEY": "local-test-placeholder"}, backend=b"genai_1::test")
            self.addCleanup(app.close)
            app.send(b"\t/mode agent\r", b"AGENT mode selected")
            app.send(b"write a draft\r", b"Agent file permission")
            app.send(b"o", b"TEMP SAVED")
            visible = "\n".join("".join(row) for row in app.screen)
            self.assertIn("[Untitled]", visible)
            self.assertIn("+ HELLO", visible)
            destination = root / "real.md"
            self.assertFalse(destination.exists())
            app.send(b"\x13", b"Save document")
            app.send(b"\x1b[200~" + str(destination).encode() + b"\x1b[201~", b"real.md")
            app.send(b"\r", "SAVED · 1 pending".encode())
            self.assertEqual(destination.read_text(), "HELLO")
            app.send(b"/changes undo\r", b"Changes undone")
            self.assertEqual(destination.read_text(), "")
            app.send(b"\x11")
            app.finish()

    def test_current_selection_is_sent_automatically_with_line_label(self):
        with tempfile.TemporaryDirectory(prefix="queditor-selection-") as directory:
            root = Path(directory)
            doc = root / "draft.md"
            doc.write_text("one\ntwo\nthree")
            config = root / "config"
            (config / "queditor").mkdir(parents=True)
            (config / "queditor/config.yaml").write_text("model: genai_1::test\n")
            provider = FakeProvider(root / "unused", deny=True)
            self.addCleanup(provider.close)
            app = TerminalApp(
                doc,
                config,
                extra_env={
                    "GENAI_1_ENDPOINT": provider.endpoint,
                    "GENAI_1_API_KEY": "local-test-placeholder",
                },
                backend=b"genai_1::test",
            )
            self.addCleanup(app.close)
            app.send(b"\x1b[1;2F", b"Selected line 1")
            app.send(b"\thelp me\r", b"Agent finished")
            request = json.dumps(provider.requests[0])
            self.assertIn("Selected line 1", request)
            self.assertIn('\\"text\\":\\"one\\"', request)
            self.assertNotIn("two\\nthree", request)
            self.assertNotIn("propose_edit", [tool["function"]["name"] for tool in provider.requests[0].get("tools", [])])
            app.send(b"\x11")
            app.finish()

    def test_connect_custom_endpoint_discovers_model_without_config_secret(self):
        with tempfile.TemporaryDirectory(prefix="queditor-connect-") as directory:
            root = Path(directory)
            doc = root / "draft.md"
            doc.write_text("")
            config = root / "config"
            provider = FakeProvider(root / "unused")
            self.addCleanup(provider.close)
            app = TerminalApp(doc, config, extra_env={"COMPANY_API_KEY": "local-test-key"})
            self.addCleanup(app.close)
            app.send(b"\t/connect\r", b"Connect provider")
            app.send(b"c", b"Provider ID")
            app.send(b"company\r", b"Provider name")
            app.send(b"Company Gateway\r", b"Base URL")
            app.send(provider.endpoint.encode() + b"\r", b"Credential source")
            app.send(b"e", b"Environment credential")
            app.send(b"COMPANY_API_KEY\r", b"Models")
            app.send(b"\x1b[B\r", b"company::test")
            saved = (config / "queditor/config.yaml").read_text()
            self.assertIn("base_url: " + provider.endpoint, saved)
            self.assertIn("env: COMPANY_API_KEY", saved)
            self.assertNotIn("local-test-key", saved)
            app.send(b"hello\r", b"Agent finished")
            self.assertEqual(len(provider.requests), 3)
            self.assertEqual(
                [
                    request["messages"][-1].get("tool_call_id")
                    for request in provider.requests[1:]
                ],
                ["call-1", "call-2"],
            )
            visible = "\n".join("".join(row) for row in app.screen)
            self.assertIn("Planner Chat · company:test", visible)
            self.assertGreaterEqual(visible.count("company:test"), 2)
            app.send(b"/models mock\r", b"Model selected and remembered")
            visible = "\n".join("".join(row) for row in app.screen)
            self.assertIn("Planner Chat · mock (offline)", visible)
            self.assertIn("company:test", visible)  # The completed reply retains its model.
            app.send(b"\x11")
            app.finish()

    def test_compaction_rejects_undeclared_tool_calls(self):
        with tempfile.TemporaryDirectory(prefix="queditor-compact-") as directory:
            root = Path(directory)
            doc = root / "draft.md"
            doc.write_text("")
            note = root / "note.md"
            note.write_text("COMPACTION_MUST_NOT_READ")
            config = root / "config"
            (config / "queditor").mkdir(parents=True)
            (config / "queditor/config.yaml").write_text("model: genai_1::test\n")
            provider = FakeProvider(note)
            self.addCleanup(provider.close)
            app = TerminalApp(doc, config, extra_env={"GENAI_1_ENDPOINT": provider.endpoint, "GENAI_1_API_KEY": "local-test-placeholder"}, backend=b"genai_1::test")
            self.addCleanup(app.close)
            app.send(b"\t/compact\r", b"[Incomplete")
            self.assertEqual(len(provider.requests), 1)
            self.assertFalse(provider.requests[0].get("tools"))
            self.assertNotIn(b"Agent file permission", app.output)
            self.assertNotIn("COMPACTION_MUST_NOT_READ", json.dumps(provider.requests))
            app.send(b"\x11")
            app.finish()

    def test_opt_in_session_resume_does_not_change_document(self):
        with tempfile.TemporaryDirectory(prefix="queditor-session-") as directory:
            root = Path(directory)
            doc = root / "draft.md"
            doc.write_text("original document")
            config = root / "config"
            (config / "queditor").mkdir(parents=True)
            (config / "queditor/config.yaml").write_text("persist_sessions: true\n")
            first = TerminalApp(doc, config)
            self.addCleanup(first.close)
            first.send(b"\tremember this conversation\r", b"Mock mode is selected")
            first.send(b"\x11")
            first.finish()
            records = list((root / "state/queditor").glob("session-*.json"))
            self.assertEqual(len(records), 1)
            identifier = records[0].stem.removeprefix("session-")
            resumed = TerminalApp(doc, config)
            self.addCleanup(resumed.close)
            resumed.send(f"\t/resume {identifier}\r".encode(), b"Session resumed")
            self.assertEqual(doc.read_text(), "original document")
            resumed.send(b"\x11")
            resumed.finish()
            self.assertEqual(len(list((root / "state/queditor").glob("session-*.json"))), 2)

    def test_signal_shutdown_restores_terminal_and_preserves_recovery(self):
        with tempfile.TemporaryDirectory(prefix="queditor-signal-") as directory:
            root = Path(directory)
            doc = root / "draft.md"
            doc.write_text("")
            app = TerminalApp(doc, root / "config")
            self.addCleanup(app.close)
            app.send(b"signal-safe text\t/keys\r", b"Profile:")
            app.process.terminate()
            app.finish()
            self.assertEqual(doc.read_text(), "")
            records = list((root / "state/queditor").glob("recovery-*.json"))
            self.assertEqual(len(records), 1)
            self.assertEqual(json.loads(records[0].read_text())["document"]["text"], "signal-safe text")

    def test_agent_mode_sends_full_document_and_saves_before_keep_review(self):
        with tempfile.TemporaryDirectory(prefix="queditor-live-") as directory:
            root = Path(directory)
            doc = root / "draft.md"
            doc.write_text("helloUNATTACHED_SECRET")
            note = root / "note.md"
            note.write_text("approved tool context")
            config = root / "config"
            (config / "queditor").mkdir(parents=True)
            (config / "queditor/config.yaml").write_text("model: genai_1::test\n")
            provider = FakeProvider(note)
            self.addCleanup(provider.close)
            app = TerminalApp(doc, config, extra_env={"GENAI_1_ENDPOINT": provider.endpoint, "GENAI_1_API_KEY": "local-test-placeholder"}, backend=b"genai_1::test")
            self.addCleanup(app.close)
            app.send(b"\x1b[1;2C" * 5 + b"\t/attach\r", b"Context snapshot attached")
            app.send(b"/mode agent\r", b"AGENT mode selected")
            app.send(b"revise\r", b"Agent file permission")
            self.assertEqual(len(provider.requests), 1)
            self.assertNotIn("approved tool context", json.dumps(provider.requests))
            app.send(b"o", b"SAVED")
            self.assertEqual(doc.read_text(), "HELLO")
            visible = "\n".join("".join(row) for row in app.screen)
            self.assertIn("\u2212 helloUNATTACHED_SECRET", visible)
            self.assertIn("+ HELLO", visible)
            self.assertIn("SAVED", visible)
            self.assertIn("Tokens in 600 out 60", visible)
            self.assertIn("in 600 out 60", visible)
            self.assertIn("tk/s", visible)
            self.assertTrue(provider.requests[0]["stream_options"]["include_usage"])
            self.assertIn("UNATTACHED_SECRET", json.dumps(provider.requests))
            self.assertIn("propose_edit", [tool["function"]["name"] for tool in provider.requests[0].get("tools", [])])
            self.assertIn("approved tool context", json.dumps(provider.requests))
            app.send(b"\x1b[13;5u", b"Changes kept")
            self.assertEqual(doc.read_text(), "HELLO")
            app.send(b"\x11")
            app.finish()
            self.assertEqual(doc.read_text(), "HELLO")
            records = list((root / "state/queditor").glob("recovery-*.json"))
            self.assertEqual(len(records), 1)
            self.assertEqual(json.loads(records[0].read_text())["document"]["text"], "helloUNATTACHED_SECRET")

    def test_agent_review_undo_saves_original_without_control_s(self):
        with tempfile.TemporaryDirectory(prefix="queditor-live-undo-") as directory:
            root = Path(directory)
            doc = root / "draft.md"
            doc.write_text("original")
            note = root / "note.md"
            note.write_text("approved tool context")
            config = root / "config"
            (config / "queditor").mkdir(parents=True)
            (config / "queditor/config.yaml").write_text("model: genai_1::test\n")
            provider = FakeProvider(note)
            self.addCleanup(provider.close)
            app = TerminalApp(doc, config, extra_env={"GENAI_1_ENDPOINT": provider.endpoint, "GENAI_1_API_KEY": "local-test-placeholder"}, backend=b"genai_1::test")
            self.addCleanup(app.close)
            app.send(b"\t/mode agent\r", b"AGENT mode selected")
            app.send(b"revise\r", b"Agent file permission")
            app.send(b"o", b"SAVED")
            self.assertEqual(doc.read_text(), "HELLO")
            app.send(b"/changes undo\r", b"Changes undone")
            self.assertEqual(doc.read_text(), "original")
            app.send(b"\x11")
            app.finish()

    def test_denied_read_never_reaches_provider(self):
        with tempfile.TemporaryDirectory(prefix="queditor-live-") as directory:
            root = Path(directory)
            doc = root / "draft.md"
            doc.write_text("")
            note = root / "note.md"
            note.write_text("DENIED_SECRET")
            config = root / "config"
            (config / "queditor").mkdir(parents=True)
            (config / "queditor/config.yaml").write_text("model: genai_1::test\n")
            provider = FakeProvider(note, deny=True)
            self.addCleanup(provider.close)
            app = TerminalApp(doc, config, extra_env={"GENAI_1_ENDPOINT": provider.endpoint, "GENAI_1_API_KEY": "local-test-placeholder"}, backend=b"genai_1::test")
            self.addCleanup(app.close)
            app.send(b"\tread the note\r", b"Agent file permission")
            app.send(b"n", b"Agent finished")
            self.assertNotIn("DENIED_SECRET", json.dumps(provider.requests))
            self.assertIn("User denied file access", json.dumps(provider.requests))
            app.send(b"\x11")
            app.finish()

    def test_crashed_document_can_be_recovered_without_writing_original(self):
        with tempfile.TemporaryDirectory(prefix="queditor-recovery-") as directory:
            root = Path(directory)
            doc = root / "draft.md"
            doc.write_text("")
            config = root / "config"
            app = TerminalApp(doc, config)
            self.addCleanup(app.close)
            app.send(b"unsaved")
            deadline = time.monotonic() + 5
            records = []
            while time.monotonic() < deadline:
                records = list((root / "state/queditor").glob("recovery-*.json"))
                if records:
                    break
                select.select([app.master], [], [], 0.1)
                if select.select([app.master], [], [], 0)[0]:
                    app.output.extend(os.read(app.master, 65536))
            self.assertTrue(records)
            app.process.kill()
            app.process.wait()
            self.assertEqual(doc.read_text(), "")
            identifier = records[0].stem.removeprefix("recovery-")
            restored = TerminalApp(doc, config)
            self.addCleanup(restored.close)
            restored.send(b"\t/recover\r", b"Choose a snapshot to restore")
            visible = "\n".join("".join(row) for row in restored.screen)
            self.assertIn(identifier[:8], visible)
            restored.send(b"\x1b")
            self.assertEqual(doc.read_text(), "")
            restored.send(b"/recover\r", b"Choose a snapshot to restore")
            restored.send(b"\r", b"Recovery restored")
            self.assertEqual(doc.read_text(), "")
            restored.send(b"\x13", b"Saved")
            restored.send(b"\x11")
            restored.finish()
            self.assertEqual(doc.read_text(), "unsaved")


if __name__ == "__main__":
    unittest.main()
