# SPDX-License-Identifier: GPL-3.0-or-later
"""Backend that drives Proton's official ``pass-cli`` via subprocesses.

Uses ``Gio.Subprocess`` + ``communicate_utf8_async`` so everything runs on
the GLib main loop without threads. Commands are serialised through a small
queue: pass-cli keeps local session state and we do not want concurrent
invocations racing on it.

Never logs stdout (it may contain secrets). stderr is only summarised to a
single line for error toasts.
"""

from __future__ import annotations

from collections import deque
from typing import Callable

from gi.repository import Gio, GLib

from .. import parsing
from ..models import ItemSummary, SessionInfo, Vault
from . import argv as A
from .base import Backend, BackendError, Callback, LoginHandle, NotLoggedIn, PassCliMissing


class _Job:
    __slots__ = ("args", "parse", "callback", "cancellable")

    def __init__(self, args, parse, callback):
        self.args = args
        self.parse = parse
        self.callback = callback
        self.cancellable = Gio.Cancellable()


class _LoginHandle(LoginHandle):
    def __init__(self, proc: Gio.Subprocess, cancellable: Gio.Cancellable):
        self._proc = proc
        self._cancellable = cancellable

    def cancel(self) -> None:
        self._cancellable.cancel()
        try:
            self._proc.force_exit()
        except GLib.Error:
            pass


class PassCliBackend(Backend):
    name = "pass-cli"

    def __init__(self) -> None:
        self._queue: deque[_Job] = deque()
        self._busy = False

    # ------------------------------------------------------------------ util

    def _binary(self) -> str:
        path = A.find_pass_cli()
        if not path:
            raise PassCliMissing(
                "pass-cli was not found. Install it from "
                "https://protonpass.github.io/pass-cli/ or set PPG_PASS_CLI."
            )
        return path

    def _launcher(self, stdin: bool = False) -> Gio.SubprocessLauncher:
        flags = Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE
        flags |= Gio.SubprocessFlags.STDIN_PIPE if stdin else Gio.SubprocessFlags.STDIN_INHERIT
        launcher = Gio.SubprocessLauncher.new(flags)
        env = A.child_env()
        launcher.set_environ([f"{k}={v}" for k, v in env.items()])
        return launcher

    def _run(self, args: list[str], parse: Callable[[str], object], callback: Callback) -> None:
        self._queue.append(_Job(args, parse, callback))
        self._pump()

    def _pump(self) -> None:
        if self._busy or not self._queue:
            return
        job = self._queue.popleft()
        self._busy = True
        try:
            binary = self._binary()
            proc = self._launcher(stdin=True).spawnv([binary, *job.args])
        except PassCliMissing as exc:
            self._finish(job, None, exc)
            return
        except GLib.Error as exc:
            self._finish(job, None, BackendError(f"Could not start pass-cli: {exc.message}"))
            return
        # stdin=None closes the pipe: pass-cli must never wait on a prompt.
        proc.communicate_utf8_async(None, job.cancellable, self._on_done, (proc, job))

    def _on_done(self, proc: Gio.Subprocess, res: Gio.AsyncResult, data) -> None:
        _proc, job = data
        try:
            _ok, stdout, stderr = proc.communicate_utf8_finish(res)
        except GLib.Error as exc:
            self._finish(job, None, BackendError(f"pass-cli failed: {exc.message}"))
            return
        stdout = stdout or ""
        stderr = stderr or ""
        if not proc.get_successful():
            if A.is_not_logged_in(stderr):
                err: BackendError = NotLoggedIn("Not signed in")
            else:
                err = BackendError(A.summarize_error(stderr))
            del stdout
            self._finish(job, None, err)
            return
        try:
            result = job.parse(stdout)
        except parsing.ParseError as exc:
            self._finish(job, None, BackendError(str(exc)))
            return
        finally:
            del stdout
        self._finish(job, result, None)

    def _finish(self, job: _Job, result, error) -> None:
        self._busy = False
        try:
            job.callback(result, error)
        finally:
            GLib.idle_add(self._pump_idle)

    def _pump_idle(self) -> bool:
        self._pump()
        return GLib.SOURCE_REMOVE

    # ------------------------------------------------------------- interface

    def session_info(self, callback: Callback) -> None:
        def done(result, error):
            if isinstance(error, NotLoggedIn):
                callback(SessionInfo(logged_in=False), None)
            else:
                callback(result, error)

        self._run(A.info(), parsing.parse_info, done)

    def login(self, on_url, callback: Callback) -> LoginHandle:
        binary = self._binary()
        cancellable = Gio.Cancellable()
        # stdin is a pipe we never write to: if pass-cli asks for the Pass
        # "extra password" it gets EOF instead of hanging on a terminal.
        proc = self._launcher(stdin=True).spawnv([binary, *A.login()])
        proc.get_stdin_pipe().close(None)
        stdout = Gio.DataInputStream.new(proc.get_stdout_pipe())
        handle = _LoginHandle(proc, cancellable)
        seen_url = {"done": False}

        def read_line(*_):
            stdout.read_line_async(GLib.PRIORITY_DEFAULT, cancellable, on_line)

        def on_line(stream, res):
            try:
                line, _len = stream.read_line_finish_utf8(res)
            except GLib.Error:
                return
            if line is None:
                return  # EOF; wait_check below reports the result
            if not seen_url["done"]:
                url = A.find_login_url(line)
                if url:
                    seen_url["done"] = True
                    on_url(url)
            read_line()

        def on_exit(p, res):
            try:
                p.wait_check_finish(res)
            except GLib.Error:
                if cancellable.is_cancelled():
                    callback(None, BackendError("Sign-in cancelled"))
                    return
                callback(None, BackendError(
                    "Sign-in did not complete. If your account uses a Pass extra "
                    "password, run “pass-cli login” once in a terminal."
                ))
                return
            self.session_info(callback)

        read_line()
        proc.wait_check_async(None, on_exit)
        return handle

    def logout(self, callback: Callback) -> None:
        self._run(A.logout(), lambda _out: True, callback)

    def list_vaults(self, callback: Callback) -> None:
        self._run(A.vault_list(), parsing.parse_vault_list, callback)

    def list_items(self, vault: Vault, callback: Callback) -> None:
        self._run(A.item_list(vault.share_id), parsing.parse_item_list, callback)

    def view_item(self, item: ItemSummary, callback: Callback) -> None:
        self._run(A.item_view(item.share_id, item.id), parsing.parse_item_view, callback)

    def get_secret(self, share_id, item_id, field_key, callback: Callback) -> None:
        self._run(
            A.item_field(share_id, item_id, field_key),
            lambda out: out[:-1] if out.endswith("\n") else out,
            callback,
        )

    def get_totp(self, share_id, item_id, field_key, callback: Callback) -> None:
        def parse(out: str) -> str:
            codes = parsing.parse_totp(out)
            if field_key in codes:
                return codes[field_key]
            if len(codes) == 1:
                return next(iter(codes.values()))
            raise parsing.ParseError("no TOTP code for that field")

        self._run(A.item_totp(share_id, item_id, field_key), parse, callback)
