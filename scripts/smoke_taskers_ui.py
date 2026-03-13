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
VIEWPORT_TOLERANCE_PX = 24


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
        ["cargo", "build", "-p", "taskers", "-p", "taskers-cli"],
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


def active_workspace_window(workspace: dict[str, Any]) -> tuple[str, dict[str, Any]]:
    window_id = workspace["active_window"]
    return window_id, workspace["windows"][window_id]


def layout_leaves(node: dict[str, Any]) -> list[str]:
    if node["kind"] == "leaf":
        return [node["pane_id"]]

    leaves = layout_leaves(node["first"])
    leaves.extend(layout_leaves(node["second"]))
    return leaves


def workspace_window_leaves(workspace: dict[str, Any]) -> list[str]:
    leaves: list[str] = []
    for window in workspace["windows"].values():
        leaves.extend(layout_leaves(window["layout"]))
    return leaves


def workspace_viewport(workspace: dict[str, Any]) -> tuple[int, int]:
    viewport = workspace.get("viewport") or {}
    return (int(viewport.get("x", 0)), int(viewport.get("y", 0)))


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
                "active_window": workspace["active_window"],
                "viewport": workspace.get("viewport", {"x": 0, "y": 0}),
                "windows": {
                    window_id: {
                        "frame": window["frame"],
                        "layout": window["layout"],
                        "active_pane": window["active_pane"],
                    }
                    for window_id, window in workspace["windows"].items()
                },
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
    expected_layout_panes = workspace_window_leaves(workspace)
    expected_layout_panes_sorted = sorted(expected_layout_panes)
    expected_layout_surface_ids = sorted(
        workspace["panes"][pane_id]["active_surface"] for pane_id in expected_layout_panes
    )
    attached_pane_cards = sorted(integrity["attached_pane_card_ids"])
    expected_window_ids = sorted(workspace["windows"].keys())
    expected_active_window_id = workspace["active_window"]
    expected_windows = sorted(
        [
            {
                "id": window_id,
                "x": window["frame"]["x"],
                "y": window["frame"]["y"],
                "width": window["frame"]["width"],
                "height": window["frame"]["height"],
                "active_pane": window["active_pane"],
                "leaf_pane_ids": layout_leaves(window["layout"]),
            }
            for window_id, window in workspace["windows"].items()
        ],
        key=lambda item: item["id"],
    )
    expected_viewport = workspace_viewport(workspace)

    if expected_active_window_id not in workspace["windows"]:
        raise AssertionError("active_window does not exist in workspace.windows")
    if workspace["active_pane"] != workspace["windows"][expected_active_window_id]["active_pane"]:
        raise AssertionError("workspace.active_pane does not match active window active_pane")
    if sorted(expected_layout_panes) != expected_workspace_panes:
        raise AssertionError("workspace window leaves do not match workspace panes")

    if integrity["active_workspace_id"] != workspace_id:
        raise AssertionError(
            f"integrity active workspace {integrity['active_workspace_id']} != {workspace_id}"
        )
    if integrity["active_workspace_window_id"] != expected_active_window_id:
        raise AssertionError(
            "integrity active workspace window id does not match model"
        )
    if sorted(integrity["active_workspace_window_ids"]) != expected_window_ids:
        raise AssertionError("integrity active workspace window ids do not match model")
    actual_windows = sorted(
        [
            {
                "id": window["id"],
                "x": window["x"],
                "y": window["y"],
                "width": window["width"],
                "height": window["height"],
                "active_pane": window["active_pane"],
                "leaf_pane_ids": window["leaf_pane_ids"],
            }
            for window in integrity["active_workspace_windows"]
        ],
        key=lambda item: item["id"],
    )
    if actual_windows != expected_windows:
        raise AssertionError("integrity active workspace windows do not match model")
    if sorted(integrity["all_live_pane_ids"]) != expected_live_panes:
        raise AssertionError("integrity all_live_pane_ids does not match model")
    if sorted(integrity["active_workspace_pane_ids"]) != expected_workspace_panes:
        raise AssertionError("integrity active_workspace_pane_ids does not match model")
    if sorted(integrity["active_layout_pane_ids"]) != expected_layout_panes_sorted:
        raise AssertionError("integrity active_layout_pane_ids does not match layout")
    if integrity["layout_host_child_count"] != 1:
        raise AssertionError(
            f"expected layout_host_child_count=1, got {integrity['layout_host_child_count']}"
        )
    if integrity["layout_root_widget_type"] is None:
        raise AssertionError("layout_root_widget_type should not be null once the shell is built")
    if attached_pane_cards != expected_layout_panes_sorted:
        raise AssertionError(
            f"attached pane cards {attached_pane_cards} != expected layout panes {expected_layout_panes_sorted}"
        )
    if not set(expected_layout_panes_sorted).issubset(integrity["cached_pane_card_ids"]):
        raise AssertionError("not all active layout panes have cached pane cards")
    actual_viewport = (
        int(integrity["viewport_x"]),
        int(integrity["viewport_y"]),
    )
    if any(
        abs(actual - expected) > VIEWPORT_TOLERANCE_PX
        for actual, expected in zip(actual_viewport, expected_viewport, strict=True)
    ):
        raise AssertionError(
            f"integrity viewport {actual_viewport} != {expected_viewport}"
        )

    cached_ghostty_surfaces = integrity["cached_ghostty_surface_ids"]
    attached_ghostty_surfaces = sorted(integrity["attached_ghostty_surface_ids"])
    if cached_ghostty_surfaces:
        if attached_ghostty_surfaces != expected_layout_surface_ids:
            raise AssertionError(
                "attached Ghostty surfaces do not match active layout surfaces"
            )
        if not set(expected_layout_surface_ids).issubset(cached_ghostty_surfaces):
            raise AssertionError("not all active layout panes have cached Ghostty surfaces")
    if (
        integrity["active_pane_focus_widget_type"] is not None
        and not integrity["active_pane_focus_has_focus"]
        and not integrity["active_pane_card_has_focus"]
    ):
        raise AssertionError("active pane does not contain GTK focus")


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


def wait_for_viewport_offset(
    ctl_bin: Path,
    socket_path: Path,
    integrity_path: Path,
    app_process: subprocess.Popen[str],
    axis: str,
    description: str,
) -> tuple[dict[str, Any], dict[str, Any]]:
    field = f"viewport_{axis}"

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
        if integrity[field] <= 0:
            return None
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
        initial_window_id, initial_window = active_workspace_window(initial_workspace)
        if len(initial_workspace["windows"]) != 1:
            raise AssertionError("newly bootstrapped workspace should have exactly one window")
        if layout_leaves(initial_window["layout"]) != [initial_workspace["active_pane"]]:
            raise AssertionError("bootstrapped workspace should have one leaf in its root window")
        if initial_window["frame"] != {"x": 0, "y": 0, "width": 1280, "height": 860}:
            raise AssertionError("bootstrapped workspace window should use the root frame")
        if workspace_viewport(initial_workspace) != (0, 0):
            raise AssertionError("bootstrapped workspace viewport should start at (0, 0)")
        print(f"Initial render OK in workspace {initial_workspace_id}")

        for split_index in range(2):
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
            status_response, integrity = wait_for_consistent_state(
                ctl_bin,
                socket_path,
                integrity_path,
                app_process,
                f"post-vertical-split-{split_index + 1} render",
            )
        model = model_from_status_response(status_response)
        _, split_workspace = active_workspace(model)
        split_window_id, split_window = active_workspace_window(split_workspace)
        if split_window_id != initial_window_id:
            raise AssertionError("vertical splits should stay within the initial top-level window")
        if len(layout_leaves(split_window["layout"])) != 3:
            raise AssertionError("expected two extra vertical splits to create three leaves")
        if workspace_viewport(split_workspace) != (0, 0):
            raise AssertionError("inner splits should not move the workspace viewport")
        split_layout_before_resize = split_window["layout"]
        print("Inner split OK")

        run_ctl(
            ctl_bin,
            socket_path,
            "pane",
            "resize-split",
            "--workspace",
            initial_workspace_id,
            "--direction",
            "down",
            "--amount",
            "60",
        )
        status_response, _ = wait_for_consistent_state(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "split resize render",
        )
        model = model_from_status_response(status_response)
        _, resized_split_workspace = active_workspace(model)
        _, resized_split_window = active_workspace_window(resized_split_workspace)
        if resized_split_window["layout"] == split_layout_before_resize:
            raise AssertionError("split resize should change the active window layout")
        print("Split resize OK")

        run_ctl(
            ctl_bin,
            socket_path,
            "pane",
            "new-window",
            "--workspace",
            initial_workspace_id,
            "--direction",
            "right",
        )
        status_response, _ = wait_for_consistent_state(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "right window creation render",
        )
        model = model_from_status_response(status_response)
        _, right_workspace = active_workspace(model)
        right_window_id, right_window = active_workspace_window(right_workspace)
        if len(right_workspace["windows"]) != 2:
            raise AssertionError("expected creating a new window to add a second top-level window")
        if len(layout_leaves(right_window["layout"])) != 1:
            raise AssertionError("new top-level windows should start with a single leaf")
        status_response, _ = wait_for_viewport_offset(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "x",
            "horizontal window auto-reveal",
        )
        model = model_from_status_response(status_response)
        _, right_workspace = active_workspace(model)
        print("Horizontal workspace window OK")

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
            "right window split render",
        )
        model = model_from_status_response(status_response)
        _, right_workspace = active_workspace(model)
        active_right_window_id, active_right_window = active_workspace_window(right_workspace)
        if active_right_window_id != right_window_id:
            raise AssertionError("split should keep focus inside the new right-hand window")
        if len(layout_leaves(active_right_window["layout"])) != 2:
            raise AssertionError("expected the right-hand window split to create a second leaf")
        remembered_right_pane = active_right_window["active_pane"]

        run_ctl(
            ctl_bin,
            socket_path,
            "pane",
            "focus-direction",
            "--workspace",
            initial_workspace_id,
            "--direction",
            "left",
        )
        status_response, _ = wait_for_consistent_state(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "focus left render",
        )
        model = model_from_status_response(status_response)
        _, left_workspace = active_workspace(model)
        if left_workspace["active_window"] == right_window_id:
            raise AssertionError("focus-direction left should leave the right-hand window")

        run_ctl(
            ctl_bin,
            socket_path,
            "pane",
            "focus-direction",
            "--workspace",
            initial_workspace_id,
            "--direction",
            "right",
        )
        status_response, _ = wait_for_consistent_state(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "focus right render",
        )
        model = model_from_status_response(status_response)
        _, refocused_workspace = active_workspace(model)
        if refocused_workspace["active_window"] != right_window_id:
            raise AssertionError("focus-direction right should restore the right-hand window")
        if refocused_workspace["active_pane"] != remembered_right_pane:
            raise AssertionError("focus-direction right should restore the remembered inner pane")
        print("Directional focus OK")

        run_ctl(
            ctl_bin,
            socket_path,
            "pane",
            "new-window",
            "--workspace",
            initial_workspace_id,
            "--direction",
            "down",
        )
        status_response, _ = wait_for_consistent_state(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "down window creation render",
        )
        model = model_from_status_response(status_response)
        _, down_workspace = active_workspace(model)
        down_window_id, down_window = active_workspace_window(down_workspace)
        if len(down_workspace["windows"]) != 3:
            raise AssertionError("expected vertical workspace window creation to add a third window")
        status_response, _ = wait_for_viewport_offset(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "y",
            "vertical window auto-reveal",
        )
        model = model_from_status_response(status_response)
        _, down_workspace = active_workspace(model)
        down_window_id, down_window = active_workspace_window(down_workspace)
        print("Vertical workspace window OK")

        original_frame = down_window["frame"].copy()
        run_ctl(
            ctl_bin,
            socket_path,
            "pane",
            "resize-window",
            "--workspace",
            initial_workspace_id,
            "--direction",
            "right",
            "--amount",
            "160",
        )
        run_ctl(
            ctl_bin,
            socket_path,
            "pane",
            "resize-window",
            "--workspace",
            initial_workspace_id,
            "--direction",
            "down",
            "--amount",
            "120",
        )
        status_response, _ = wait_for_consistent_state(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "window resize render",
        )
        model = model_from_status_response(status_response)
        _, resized_window_workspace = active_workspace(model)
        _, resized_window = active_workspace_window(resized_window_workspace)
        if resized_window["frame"]["width"] <= original_frame["width"]:
            raise AssertionError("window resize should increase the active window width")
        if resized_window["frame"]["height"] <= original_frame["height"]:
            raise AssertionError("window resize should increase the active window height")
        print("Window resize OK")

        run_ctl(
            ctl_bin,
            socket_path,
            "pane",
            "close",
            "--workspace",
            initial_workspace_id,
            "--pane",
            resized_window_workspace["active_pane"],
        )
        status_response, _ = wait_for_consistent_state(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "post-window-close render",
        )
        model = model_from_status_response(status_response)
        _, closed_workspace = active_workspace(model)
        if down_window_id in closed_workspace["windows"]:
            raise AssertionError("closing the only pane in a top-level window should remove that window")
        if len(closed_workspace["windows"]) != 2:
            raise AssertionError("expected closing the one-pane window to leave two windows")
        persisted_initial_viewport = workspace_viewport(closed_workspace)
        print("Workspace window close OK")

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
            other_workspace = model["workspaces"][other_workspace_id]
            if len(other_workspace["windows"]) != 1:
                raise AssertionError("new workspaces should start with one top-level window")

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
        status_response, integrity = wait_for_consistent_state(
            ctl_bin,
            socket_path,
            integrity_path,
            app_process,
            "switch back render",
        )
        model = model_from_status_response(status_response)
        if active_workspace_id(model) != initial_workspace_id:
            raise AssertionError("switching back did not restore the original workspace")
        restored_viewport = (
            int(integrity["viewport_x"]),
            int(integrity["viewport_y"]),
        )
        if any(
            abs(actual - expected) > VIEWPORT_TOLERANCE_PX
            for actual, expected in zip(
                restored_viewport, persisted_initial_viewport, strict=True
            )
        ):
            raise AssertionError("switching back did not restore the saved viewport")
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

        terminate_process(app_process, "taskers-app")
        app_process = None
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
        restored_session_structure = structural_model(read_json(session_path)["model"])
        if restarted_structure != restored_session_structure:
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
