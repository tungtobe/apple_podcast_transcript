#!/usr/bin/env python3
"""List Gemini models that support generateContent."""

import json
import os
import sys
import urllib.error
import urllib.request


def emit(obj: dict):
    print(json.dumps(obj, ensure_ascii=False), flush=True)


def main():
    api_key = os.environ.get("GEMINI_API_KEY", "").strip()
    if not api_key:
        emit({"error": "Gemini API key is required to load models."})
        sys.exit(1)

    request = urllib.request.Request(
        "https://generativelanguage.googleapis.com/v1beta/models",
        headers={"x-goog-api-key": api_key},
    )

    try:
        with urllib.request.urlopen(request, timeout=15) as response:
            body = response.read().decode("utf-8")
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace")
        emit({"error": f"Google API returned {e.code}: {body[:300]}"})
        sys.exit(1)
    except Exception as e:
        emit({"error": f"Failed to load Gemini models: {e}"})
        sys.exit(1)

    try:
        data = json.loads(body)
    except json.JSONDecodeError as e:
        emit({"error": f"Invalid models response: {e}"})
        sys.exit(1)

    models = []
    for model in data.get("models", []):
        methods = model.get("supportedGenerationMethods", [])
        if "generateContent" not in methods:
            continue
        name = model.get("name", "")
        if name.startswith("models/"):
            name = name[len("models/"):]
        if name:
            models.append(name)

    emit({"models": sorted(set(models))})


if __name__ == "__main__":
    main()
