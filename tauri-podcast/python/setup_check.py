#!/usr/bin/env python3
"""
Dependency checker — outputs a single JSON object to stdout.
{"python_ok": true, "python_version": "3.11.2",
 "ffmpeg_ok": true, "missing_packages": []}
"""
import json
import subprocess
import sys


def check_python():
    v = sys.version_info
    return v >= (3, 10), f"{v.major}.{v.minor}.{v.micro}"


def check_ffmpeg():
    try:
        r = subprocess.run(
            ["ffmpeg", "-version"],
            capture_output=True,
            timeout=5
        )
        return r.returncode == 0
    except (FileNotFoundError, subprocess.TimeoutExpired):
        return False


def check_package(import_name: str) -> bool:
    """Check if a package can be imported."""
    try:
        __import__(import_name)
        return True
    except ImportError:
        return False


# Map: pip name → import name
REQUIRED = [
    ("faster-whisper", "faster_whisper"),
    ("google-generativeai", "google.generativeai"),
    ("python-dotenv", "dotenv"),
]

python_ok, python_version = check_python()
ffmpeg_ok = check_ffmpeg()
missing_packages = [
    pip_name
    for pip_name, import_name in REQUIRED
    if not check_package(import_name)
]

print(json.dumps({
    "python_ok": python_ok,
    "python_version": python_version,
    "ffmpeg_ok": ffmpeg_ok,
    "missing_packages": missing_packages,
}, ensure_ascii=False), flush=True)
