import unittest

from transcriber import parse_gemini_segments


class ParseGeminiSegmentsTests(unittest.TestCase):
    def test_parses_json_array_with_markdown_fence(self):
        raw = """```json
[{"start": 1.0, "end": 2.0, "text": "hello"}]
```"""

        self.assertEqual(
            parse_gemini_segments(raw),
            [{"start": 1.0, "end": 2.0, "text": "hello"}],
        )

    def test_parses_first_json_array_when_response_has_extra_text(self):
        raw = """
Here is [the transcript]:
[{"start": 1.0, "end": 2.0, "text": "hello"}]
Done.
"""

        self.assertEqual(
            parse_gemini_segments(raw),
            [{"start": 1.0, "end": 2.0, "text": "hello"}],
        )

    def test_parses_object_with_segments_key(self):
        raw = '{"segments": [{"start": 1, "end": 2, "text": "hello"}]}'

        self.assertEqual(
            parse_gemini_segments(raw),
            [{"start": 1.0, "end": 2.0, "text": "hello"}],
        )

    def test_accepts_raw_newline_inside_text(self):
        raw = '[{"start": 1, "end": 2, "text": "hello\nworld"}]'

        self.assertEqual(
            parse_gemini_segments(raw),
            [{"start": 1.0, "end": 2.0, "text": "hello\nworld"}],
        )


if __name__ == "__main__":
    unittest.main()
