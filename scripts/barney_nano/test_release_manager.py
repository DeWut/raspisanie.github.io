from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import release_manager


class ReleaseManagerTests(unittest.TestCase):
    def test_format_bytes(self):
        self.assertEqual(release_manager.format_bytes(None), "н/д")
        self.assertIn("КиБ", release_manager.format_bytes(1024))

    def test_dataset_stats_counts_valid_examples_and_duplicates(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "approved.jsonl"
            rows = [
                {"messages": [
                    {"role": "user", "content": "Объясни API"},
                    {"role": "assistant", "content": "API — интерфейс взаимодействия программ."},
                ]},
                {"messages": [
                    {"role": "user", "content": "  Объясни   API "},
                    {"role": "assistant", "content": "API позволяет программам общаться."},
                ]},
                {"messages": [{"role": "user", "content": "Без ответа"}]},
            ]
            path.write_text(
                "\n".join(json.dumps(row, ensure_ascii=False) for row in rows) + "\n",
                encoding="utf-8",
            )
            result = release_manager.dataset_stats(path)
            self.assertEqual(result["examples"], 2)
            self.assertEqual(result["unique_user_prompts"], 1)
            self.assertEqual(result["invalid_lines"], 1)
            self.assertIsNotNone(result["sha256"])

    def test_adapter_hash_changes_when_weights_change(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            weights = folder / "adapter_model.safetensors"
            weights.write_bytes(b"first")
            first_hash = release_manager.adapter_weights_hash(folder)
            weights.write_bytes(b"second")
            second_hash = release_manager.adapter_weights_hash(folder)
            self.assertIsNotNone(first_hash)
            self.assertNotEqual(first_hash, second_hash)

    def test_benchmark_stats_requires_structured_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "benchmark.json"
            path.write_text(json.dumps({
                "cases": [
                    {"candidate_answer": "Ответ", "candidate_tokens_per_second": 12.0, "base_tokens_per_second": 16.0},
                    {"candidate_answer": "", "candidate_tokens_per_second": 8.0, "base_tokens_per_second": 8.0},
                    {"candidate_answer": "Ещё ответ"},
                ]
            }), encoding="utf-8")
            stats = release_manager.benchmark_stats(path)
            self.assertTrue(stats["exists"])
            self.assertEqual(stats["cases"], 3)
            self.assertEqual(stats["non_empty"], 2)
            self.assertEqual(stats["avg_candidate_tokens_per_second"], 10.0)
            self.assertEqual(stats["avg_base_tokens_per_second"], 12.0)


if __name__ == "__main__":
    unittest.main()
