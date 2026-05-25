#!/usr/bin/env python3
"""
CLI Meeting Memo Generator — uses Gemini to create Japanese meeting notes.
Outputs newline-delimited JSON events to stdout.

Usage:
  python memo_generator.py \\
    --transcript <path-to-json> \\
    --api-key <gemini-key> \\
    --model <gemini-model> \\
    [--prompt-template <template>] \\
    --output <output-txt-path> \\
    [--force-rerun]

Event types:
  {"type":"result","content":"...","cached":false}
  {"type":"error","message":"..."}
"""

import argparse
import json
import os
import sys


def emit(obj: dict):
    print(json.dumps(obj, ensure_ascii=False), flush=True)


MEMO_PROMPT_TEMPLATE = """あなたは議事録作成のプロフェッショナルです。
会議やポッドキャストの内容から、以下のフォーマットで日本語のメモを作成してください：

## 主な内容
* [トピック1]
   * 詳細なポイント、重要な発言、具体的な内容
   * 関連する情報やメモ
* [トピック2]
   * 詳細なポイント

## Next Action
* 具体的なアクションアイテムがあればリストアップ
* 担当者や期限が言及されていれば記載

## まとめ
全体の要約と重要なポイントを簡潔にまとめる

箇条書きを効果的に使用し、読みやすく構造化してください。

以下のトランスクリプトから議事録メモを作成してください：

{transcript}"""


def main():
    parser = argparse.ArgumentParser(description="Meeting memo generator")
    parser.add_argument("--transcript", required=True, help="Path to transcript JSON")
    parser.add_argument("--api-key", required=True)
    parser.add_argument("--model", default="gemini-2.0-flash")
    parser.add_argument("--prompt-template", default=MEMO_PROMPT_TEMPLATE)
    parser.add_argument("--output", required=True, help="Output path for memo .txt")
    parser.add_argument("--force-rerun", action="store_true")
    args = parser.parse_args()

    # Return cached memo if exists
    if os.path.exists(args.output) and not args.force_rerun:
        with open(args.output, encoding="utf-8") as f:
            content = f.read()
        emit({"type": "result", "content": content, "cached": True})
        return

    # Load transcript
    if not os.path.exists(args.transcript):
        emit({"type": "error", "message": f"Transcript file not found: {args.transcript}"})
        sys.exit(1)

    with open(args.transcript, encoding="utf-8") as f:
        data = json.load(f)

    # Handle both {segments: [...]} and plain [...] format
    if isinstance(data, list):
        segments = data
    else:
        segments = data.get("segments", [])

    if not segments:
        emit({"type": "error", "message": "No segments found in transcript."})
        sys.exit(1)

    full_text = " ".join(s["text"] for s in segments if s.get("text"))

    # Generate with Gemini
    try:
        import google.generativeai as genai
        genai.configure(api_key=args.api_key)
        model = genai.GenerativeModel(args.model)
        prompt_template = args.prompt_template.strip() or MEMO_PROMPT_TEMPLATE
        if "{transcript}" in prompt_template:
            prompt = prompt_template.replace("{transcript}", full_text)
        else:
            prompt = f"{prompt_template}\n\nTranscript:\n{full_text}"
        response = model.generate_content(prompt)
        content = response.text

        # Save to output file
        os.makedirs(os.path.dirname(args.output) or ".", exist_ok=True)
        with open(args.output, "w", encoding="utf-8") as f:
            f.write(content)

        emit({"type": "result", "content": content, "cached": False})

    except Exception as e:
        emit({"type": "error", "message": str(e)})
        sys.exit(1)


if __name__ == "__main__":
    main()
