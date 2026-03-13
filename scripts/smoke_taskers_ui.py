#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

sys.dont_write_bytecode = True

REPO_ROOT = Path(__file__).resolve().parent.parent
TARGET_DIR = REPO_ROOT / "target" / "debug"
POLL_INTERVAL_SECONDS = 0.1
WAIT_TIMEOUT_SECONDS = 30.0


def run_command(
    args: list[str],
    *,
    cwd: Path = REPO_ROOT,
    env: dict[str, str] | None = None,
    capture_output: bool = True,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=cwd,
        env=env,
        text=True,
        capture_output=capture_output,
        check=True,
    )


def wait_for(
    description: str,
    predicate,
    *,
    timeout_seconds: float = WAIT_TIMEOUT_SECONDS,
) -> Any:
    deadline = time.monotonic() + timeout_seconds
    last_error: Exception | None = None

    while time.monotonic() < deadline:
        try:
            result = predicate()
            if result:
                return result
        except Exception as error:  # noqa: PERF203 - retain last failure context
            last_error = error
        time.sleep(POLL_INTERVAL_SECONDS)

    if last_error is not None:
        raise RuntimeError(f"timed out waiting for {description}: {last_error}") from last_error
    raise RuntimeError(f"timed out waiting for {description}")


def terminate_process(process: subprocess.Popen[str] | None, name: str) -> None:
    if process is None or process.poll() is not None:
        return

    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return

    try:
        process.wait(timeout=5)
        return
    except subprocess.TimeoutExpired:
        pass

    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        return
    process.wait(timeout=5)


def choose_display_number() -> int:
    for display_number in range(99, 120):
        socket_path = Path("/tmp/.X11-unix") / f"X{display_number}"
        if not socket_path.exists():
            return display_number
    raise RuntimeError("could not find a free X display number between :99 and :119")


def launch_xvfb(temp_dir: Path) -> tuple[str, subprocess.Popen[str]]:
    xvfb = shutil.which("Xvfb")
    if xvfb is None:
        raise RuntimeError("Xvfb is required for the smoke harness but was not found in PATH")

    display_number = choose_display_number()
    display = f":{display_number}"
    log_path = temp_dir / "xvfb.log"
    log_file = log_path.open("w", encoding="utf-8")
    process = subprocess.Popen(
        [xvfb, display, "-screen", "0", "1440x960x24"],
        cwd=REPO_ROOT,
        stdout=log_file,
        stderr=subprocess.STDOUT,
        text=True,
        start_new_session=True,
    )

    socket_path = Path("/tmp/.X11-unix") / f"X{display_number}"
    try:
        wait_for(
            f"Xvfb on {display}",
            lambda: process.poll() is None and socket_path.exists(),
            timeout_seconds=5,
        )
    except Exception:
        terminate_process(process, "Xvfb")
        raise

    return display, process


def build_binaries() -> None:
    run_command(
        ["cargo", "build", "-p", "taskers-app", "-p", "taskers-cli"],
        capture_output=False,
    )


def launch_taskers_app(
    app_bin: Path,
    *,
    display: str,
    socket_path: Path,
    session_path: Path,
    integrity_path: Path,
    log_path: Path,
) -> subprocess.Popen[str]:
    socket_path.unlink(missing_ok=True)
    integrity_path.unlink(missing_ok=True)

    env = os.environ.copy()
    env["DISPLAY"] = display
    env["TASKERS_UI_INTEGRITY_PATH"] = str(integrity_path)

    log_file = log_path.open("a", encoding="utf-8")
    print("\n=== taskers-app launch ===", file=log_file)
    log_file.flush()

    return subprocess.Popen(
        [
            str(app_bin),
            "--demo",
            "--socket",
            str(socket_path),
            "--session",
            str(session_path),
        ],
        cwd=REPO_ROOT,
        env=env,
        stdout=log_file,
        stderr=subprocess.STDOUT,
        text=True,
        start_new_session=True,
    )


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def run_ctl(ctl_bin: Path, socket_path: Path, *args: str) -> dict[str, Any]:
    completed = run_command([str(ctl_bin), *args, "--socket", str(socket_path)])
    return json.loads(completed.stdout)


def model_from_status_response(response: dict[str, Any]) -> dict[str, Any]:
    return response["response"]["Ok"]["session"]["model"]


def active_workspace_id(model: dict[str, Any]) -> str:
    active_window_id = model["active_window"]
    return model["windows"][active_window_id]["active_workspace"]


def active_workspace(model: dict[str, Any]) -> tuple[str, dict[str, Any]]:
    workspace_id = active_workspace_id(model)
    return workspace_id, model["workspaces"][workspace_id]


def layout_leaves(node: dict[str, Any]) -> list[str]:
    if node["kind"] == "leaf":
        return [node["pane_id"]]

    leaves = layout_leaves(node["first"])
    leaves.extend(layout_leaves(node["second"]))
    return leaves


def structural_model(model: dict[str, Any]) -> dict[str, Any]:
    return {
        "active_window": model["active_window"],
        "windows": {
            window_id: {
                "workspace_order": window["workspace_order"],
                "active_workspace": window["active_workspace"],
            }
            for window_id, window in model["windows"].items()
        },
        "workspaces": {
            workspace_id: {
                "label": workspace["label"],
                "layout": workspace["layout"],
                "pane_ids": sorted(workspace["panes"].keys()),
                "active_pane": workspace["active_pane"],
            }
            for workspace_id, workspace in model["workspaces"].items()
        },
    }


def assert_integrity_matches_model(model: dict[str, Any], integrity: dict[str, Any]) -> None:
    workspace_id, workspace = active_workspace(model)
    expected_live_panes = sorted(
        pane_id
        for candidate in model["workspaces"].values()
        for pane_id in candidate["panes"].keys()
    )
    expected_workspace_panes = sorted(workspace["panes"].keys())
    expected_layout_panes = sorted(layout_leaves(workspace["layout"]))
    attached_pane_cards = sorted(integrity["attached_pane_card_ids"])

    if integrity["active_workspace_id"] != workspace_id:
        raise AssertionError(
            f"integrity active workspace {integrity['active_workspace_id']} != {workspace_id}"
        )
    if sorted(integrity["all_live_pane_ids"]) != expected_live_panes:
        raise AssertionError("integrity all_live_pane_ids does not match model")
    if sorted(integrity["active_workspace_pane_ids"]) != expected_workspace_panes:
        raise AssertionError("integrity active_workspace_pane_ids does not match model")
    if sorted(integrity["active_layout_pane_ids"]) != expected_layout_panes:
        raise AssertionError("integrity active_layout_pane_ids does not match layout")
    if integrity["layout_host_child_count"] != 1:
        raise AssertionError(
            f"expected layout_host_child_count=1, got {integrity['layout_host_child_count']}"
        )
    if integrity["layout_root_widget_type"] is None:
        raise AssertionError("layout_root_widget_type should not be null once the shell is built")
    if attached_pane_cards != expected_layout_panes:
        raise AssertionError(
            f"attached pane cards {attached_pane_cards} != expected layout panes {expected_layout_panes}"
        )
    if not set(expected_layout_panes).issubset(integrity["cached_pane_card_ids"]):
        raise AssertionError("not all active layout panes have cached pane cards")

    cached_ghostty_surfaces = integrity["cached_ghostty_surface_ids"]
    attached_ghostty_surfaces = sorted(integrity["attached_ghostty_surface_ids"])
    if cached_ghostty_surfaces:
        if attached_ghostty_surfaces != expected_layout_panes:
            raise AssertionError(
                "attached Ghostty surfaces do not match active layout panes"
            )
        if not set(expected_layout_panes).issubset(cached_ghostty_surfaces):
            raise AssertionError("not all active layout panes have cached Ghostty surfaces")


def wait_for_consistent_state(
    ctl_bin: Path,
    socket_path: Path,
    integrity_path: Path,
    app_process: subprocess.Popen[str],
    description: str,
) -> tuple[dict[str, Any], dict[str, Any]]:
    def predicate() -> tuple[dict[str, Any], dict[str, Any]] | None:
        if app_process.poll() is not None:
            raise RuntimeError(
                f"taskers-app exited early with status {app_process.returncode}"
            )
        if not socket_path.exists() or not integrity_path.exists():
            return None

        response = run_ctl(ctl_bin, socket_path, "query", "status")
        model = model_from_status_response(response)
        integrity = read_json(integrity_path)
        assert_integrity_matches_model(model, integrity)
        return response, integrity

    return wait_for(description, predicate)


def tail_lines(path: Path, count: int = 60) -> str:
    if not path.exists():
        return ""
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    return "\n".join(lines[-count:])


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Run the taskers GTK/Ghostty smoke harness."
    )
    parser.add_argument(
        "--display",
        help="Use an existing X display instead of launching Xvfb.",
    )
    parser.add_argument(
        "--skip-build",
        action="store_true",
        help="Reuse existing debug binaries instead of rebuilding them first.",
    )
    parser.add_argument(
        "--keep-temp",
        action="store_true",
        help="Keep the temporary logs and session artifacts after the run.",
    )
    args = parser.parse_args()

    temp_dir = Path(tempfile.mkdtemp(prefix="taskers-smoke-"))

    xvfb_process: subprocess.Popen[str] | None = None
    app_process: subprocess.Popen[str] | None = None

    try:
        if not args.skip_build:
            build_binaries()

        app_bin = TARGET_DIR / "taskers"
        ctl_bin = TARGET_DIR / "taskersctl"
        if not app_bin.exists():
            raise RuntimeError(f"expected app binary at {app_bin}")
        if not ctl_bin.exists():
            raise RuntimeError(f"expected control binary at {ctl_bin}")

        if args.display:
            display = args.display
        else:
            display, xvfb_process = launch_xvfb(temp_dir)

        socket_path = temp_dir / "taskers.sock"
        session_path = temp_dir / "session.json"
        integrity_path = temp_dir / "ui-integrity.json"
        app_log_path = temp_dir / "app.log"

        app_process = launch_taskers_app(
            app_bin,
            display=display,
            socket_path=socket_path,
            session_path=session_path,
            integrity_path=integrity_path,
            log_path=app_log_path,
        )

        status_response, _ = wait_for_consistent_state(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "initial taskers render",
        )
        model = model_from_status_response(status_response)
        initial_workspace_id, initial_workspace = active_workspace(model)
        initial_pane_count = len(layout_leaves(initial_workspace["layout"]))
        print(f"Initial render OK in workspace {initial_workspace_id}")

        run_ctl(
            ctl_bin,
            socket_path,
            "pane",
            "split",
            "--workspace",
            initial_workspace_id,
            "--axis",
            "vertical",
        )
        status_response, _ = wait_for_consistent_state(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "post-split layout render",
        )
        model = model_from_status_response(status_response)
        _, split_workspace = active_workspace(model)
        split_pane_count = len(layout_leaves(split_workspace["layout"]))
        if split_pane_count != initial_pane_count + 1:
            raise AssertionError(
                f"expected pane count {initial_pane_count + 1} after split, got {split_pane_count}"
            )
        print("Pane split OK")

        run_ctl(
            ctl_bin,
            socket_path,
            "pane",
            "close",
            "--workspace",
            initial_workspace_id,
            "--pane",
            split_workspace["active_pane"],
        )
        status_response, _ = wait_for_consistent_state(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "post-close layout render",
        )
        model = model_from_status_response(status_response)
        _, closed_workspace = active_workspace(model)
        closed_pane_count = len(layout_leaves(closed_workspace["layout"]))
        if closed_pane_count != initial_pane_count:
            raise AssertionError(
                f"expected pane count {initial_pane_count} after close, got {closed_pane_count}"
            )
        print("Pane close OK")

        window = model["windows"][model["active_window"]]
        other_workspace_id = next(
            (
                workspace_id
                for workspace_id in window["workspace_order"]
                if workspace_id != initial_workspace_id
            ),
            None,
        )
        if other_workspace_id is None:
            response = run_ctl(
                ctl_bin,
                socket_path,
                "workspace",
                "new",
                "--label",
                "Smoke Workspace",
            )
            other_workspace_id = response["response"]["Ok"]["workspace_id"]
            status_response, _ = wait_for_consistent_state(
                ctl_bin,
                socket_path,
                integrity_path,
                app_process,
                "new workspace render",
            )
            model = model_from_status_response(status_response)

        run_ctl(
            ctl_bin,
            socket_path,
            "workspace",
            "switch",
            "--workspace",
            other_workspace_id,
        )
        status_response, _ = wait_for_consistent_state(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "workspace switch render",
        )
        model = model_from_status_response(status_response)
        if active_workspace_id(model) != other_workspace_id:
            raise AssertionError("workspace switch did not change the active workspace")
        print("Workspace switch OK")

        run_ctl(
            ctl_bin,
            socket_path,
            "workspace",
            "switch",
            "--workspace",
            initial_workspace_id,
        )
        status_response, _ = wait_for_consistent_state(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "switch back render",
        )
        model = model_from_status_response(status_response)
        if active_workspace_id(model) != initial_workspace_id:
            raise AssertionError("switching back did not restore the original workspace")
        print("Workspace switch-back OK")

        run_ctl(
            ctl_bin,
            socket_path,
            "workspace",
            "close",
            "--workspace",
            other_workspace_id,
        )
        status_response, _ = wait_for_consistent_state(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "workspace close render",
        )
        model = model_from_status_response(status_response)
        if other_workspace_id in model["workspaces"]:
            raise AssertionError("workspace close did not remove the target workspace")
        print("Workspace close OK")

        expected_structure = structural_model(model)
        terminate_process(app_process, "taskers-app")
        app_process = launch_taskers_app(
            app_bin,
            display=display,
            socket_path=socket_path,
            session_path=session_path,
            integrity_path=integrity_path,
            log_path=app_log_path,
        )
        status_response, _ = wait_for_consistent_state(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "post-restart render",
        )
        restarted_structure = structural_model(model_from_status_response(status_response))
        if restarted_structure != expected_structure:
            raise AssertionError("session restore did not preserve the workspace structure")
        print("Session restore OK")

        print("taskers UI smoke test passed")
        if args.keep_temp:
            print(f"Artifacts preserved at {temp_dir}")
        return 0
    except Exception as error:
        print(f"taskers UI smoke test failed: {error}", file=sys.stderr)
        print(f"Artifacts preserved at {temp_dir}", file=sys.stderr)

        app_log = temp_dir / "app.log"
        if app_log.exists():
            print("--- taskers-app log tail ---", file=sys.stderr)
            print(tail_lines(app_log), file=sys.stderr)

        xvfb_log = temp_dir / "xvfb.log"
        if xvfb_log.exists():
            print("--- Xvfb log tail ---", file=sys.stderr)
            print(tail_lines(xvfb_log), file=sys.stderr)
        return 1
    finally:
        terminate_process(app_process, "taskers-app")
        terminate_process(xvfb_process, "Xvfb")
        if not args.keep_temp:
            shutil.rmtree(temp_dir, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
