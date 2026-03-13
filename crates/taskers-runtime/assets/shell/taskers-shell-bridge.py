#!/usr/bin/env python3
import argparse
import json
import os
import socket
import sys
import uuid
from datetime import datetime, timezone


def env_required(name: str) -> str | None:
    value = os.environ.get(name, "").strip()
    return value or None


def parse_optional_bool(value: str | None) -> bool | None:
    if value is None:
        return None
    normalized = value.strip().lower()
    if normalized in {"1", "true", "yes", "on"}:
        return True
    if normalized in {"0", "false", "no", "off"}:
        return False
    raise argparse.ArgumentTypeError(f"invalid boolean value: {value}")


def send_request(command: dict) -> int:
    socket_path = env_required("TASKERS_SOCKET")
    if not socket_path:
        return 0

    request = {
        "request_id": str(uuid.uuid4()),
        "command": command,
    }
    payload = json.dumps(request, separators=(",", ":")).encode("utf-8") + b"\n"

    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
            client.connect(socket_path)
            client.sendall(payload)
            client.shutdown(socket.SHUT_WR)
            client.makefile("rb").readline()
    except OSError:
        return 0

    return 0


def signal_command(args: argparse.Namespace) -> int:
    workspace_id = env_required("TASKERS_WORKSPACE_ID")
    pane_id = env_required("TASKERS_PANE_ID")
    surface_id = env_required("TASKERS_SURFACE_ID")
    if not workspace_id or not pane_id:
        return 0

    metadata = {
        "title": args.title,
        "cwd": args.cwd,
        "repo_name": args.repo,
        "git_branch": args.branch,
        "ports": [],
        "agent_kind": args.agent,
        "agent_active": args.agent_active,
    }
    metadata_payload = (
        metadata
        if any(value is not None for key, value in metadata.items() if key != "ports")
        else None
    )

    command = {
        "command": "emit_signal",
        "workspace_id": workspace_id,
        "pane_id": pane_id,
        "surface_id": surface_id,
        "event": {
            "source": "shell",
            "kind": args.kind,
            "message": args.message,
            "metadata": metadata_payload,
            "timestamp": datetime.now(timezone.utc).isoformat(),
        },
    }
    return send_request(command)


def notify_command(args: argparse.Namespace) -> int:
    workspace_id = env_required("TASKERS_WORKSPACE_ID")
    pane_id = env_required("TASKERS_PANE_ID")
    surface_id = env_required("TASKERS_SURFACE_ID")
    if not workspace_id or not pane_id:
        return 0

    title = args.title.strip()
    body = (args.body or "").strip()
    message = body or title
    metadata = {
        "title": title or None,
        "cwd": None,
        "repo_name": None,
        "git_branch": None,
        "ports": [],
        "agent_kind": args.agent,
    }
    metadata_payload = (
        metadata
        if any(value is not None for key, value in metadata.items() if key != "ports")
        else None
    )

    command = {
        "command": "emit_signal",
        "workspace_id": workspace_id,
        "pane_id": pane_id,
        "surface_id": surface_id,
        "event": {
            "source": f"notify:{title}",
            "kind": "notification",
            "message": message or None,
            "metadata": metadata_payload,
            "timestamp": datetime.now(timezone.utc).isoformat(),
        },
    }
    return send_request(command)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(add_help=False)
    subparsers = parser.add_subparsers(dest="command")

    signal = subparsers.add_parser("signal", add_help=False)
    signal.add_argument("--kind", required=True)
    signal.add_argument("--message")
    signal.add_argument("--title")
    signal.add_argument("--cwd")
    signal.add_argument("--repo")
    signal.add_argument("--branch")
    signal.add_argument("--agent")
    signal.add_argument("--agent-active", type=parse_optional_bool)
    signal.set_defaults(func=signal_command)

    notify = subparsers.add_parser("notify", add_help=False)
    notify.add_argument("--title", required=True)
    notify.add_argument("--body")
    notify.add_argument("--agent")
    notify.set_defaults(func=notify_command)

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    if not hasattr(args, "func"):
        return 0
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
