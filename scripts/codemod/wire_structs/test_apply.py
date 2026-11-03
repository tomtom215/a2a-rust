# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
import sys, unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import apply as A  # noqa: E402


def run_rs(text, path="crates/x/src/a.rs"):
    stats = {"converted": 0, "patterns": 0, "skipped": [], "files": []}
    return A.process_rs(path, text, stats), stats


class FenceTests(unittest.TestCase):
    def test_code_between_doc_blocks_is_untouched_and_not_prefixed(self):
        src = (
            "/// Docs.\n"
            "///\n"
            "/// ```rust\n"
            "/// let p = TaskIdParams { id: x, tenant: None };\n"
            "/// ```\n"
            "\n"
            "use std::collections::HashMap;\n"
            "use std::future::Future;\n"
            "\n"
            "/// More.\n"
            "///\n"
            "/// ```\n"
            "/// let q = 1;\n"
            "/// ```\n"
            "fn f() {}\n"
        )
        out, stats = run_rs(src)
        self.assertEqual(out, src.replace("TaskIdParams { id: x, tenant: None }", "TaskIdParams::new(x)"))
        self.assertEqual(stats["converted"], 1)

    def test_a_non_rust_fence_is_skipped_whole(self):
        src = "/// ```text\n/// TaskIdParams { id: x }\n/// ```\n/// ```rust\n/// let p = TaskIdParams { id: x, tenant: None };\n/// ```\nfn f() {}\n"
        out, _ = run_rs(src)
        self.assertEqual(out, src.replace("let p = TaskIdParams { id: x, tenant: None }", "let p = TaskIdParams::new(x)"))

    def test_an_unclosed_fence_leaves_the_following_code_alone(self):
        src = "/// ```rust\n/// let p = TaskIdParams { id: x, tenant: None };\nfn f() { let q = 1; }\n"
        out, _ = run_rs(src)
        self.assertEqual(out, src)

    def test_markdown_fences_and_blank_lines(self):
        stats = {"converted": 0, "patterns": 0, "skipped": [], "files": []}
        md = "text\n\n```rust\nlet p = TaskIdParams { id: x, tenant: None };\n\nlet z = 2;\n```\n\nmore\n```text\nTaskIdParams { id: x }\n```\n"
        out = "\n".join(A.convert_fences(md.split("\n"), False, stats, "x.md"))
        self.assertEqual(out, md.replace("TaskIdParams { id: x, tenant: None }", "TaskIdParams::new(x)"))

    def test_in_crate_code_is_not_converted_but_its_doc_examples_are(self):
        src = "fn f() { let a = TaskIdParams { id: x, tenant: None }; }\n/// ```\n/// let b = TaskIdParams { id: x, tenant: None };\n/// ```\nfn g() {}\n"
        out, _ = run_rs(src, path="crates/a2a-protocol-types/src/b.rs")
        self.assertIn("let a = TaskIdParams { id: x, tenant: None };", out)
        self.assertIn("/// let b = TaskIdParams::new(x);", out)


class RustdocWordTests(unittest.TestCase):
    def test_no_run_should_panic_and_compile_fail_fences_are_rust(self):
        for word in ("no_run", "should_panic", "compile_fail", "ignore", "edition2021", "rust,no_run", "rust"):
            src = f"/// ```{word}\n/// let p = TaskIdParams {{ id: x, tenant: None }};\n/// ```\nfn f() {{}}\n"
            out, _ = run_rs(src)
            self.assertIn("TaskIdParams::new(x)", out, word)
        out, _ = run_rs("/// ```text\n/// TaskIdParams { id: x, tenant: None }\n/// ```\nfn f() {}\n")
        self.assertIn("TaskIdParams { id: x, tenant: None }", out)


if __name__ == "__main__":
    unittest.main()
