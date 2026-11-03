# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from rust_literal import FieldSpec as F, TypeSpec, convert, mask, match_close, split_fields  # noqa: E402

SPECS = {
    "TaskQueryParams": TypeSpec("TaskQueryParams", [F("id", "req"), F("tenant", "opt"), F("history_length", "opt")]),
    "MessageSendParams": TypeSpec("MessageSendParams", [F("message", "req"), F("tenant", "opt"), F("configuration", "opt"), F("metadata", "opt")]),
    "AuthorizationCodeFlow": TypeSpec("AuthorizationCodeFlow", [F("authorization_url", "req"), F("token_url", "req"), F("refresh_url", "opt"), F("scopes", "coll"), F("pkce_required", "opt")]),
    "StringList": TypeSpec("StringList", [F("list", "req")]),
    "TaskIdParams": TypeSpec("TaskIdParams", [F("id", "req"), F("tenant", "opt")]),
}


def run(src):
    return convert(src, SPECS)


class MaskTests(unittest.TestCase):
    def test_strings_and_comments_are_blanked(self):
        src = 'let a = "TaskQueryParams { id: 1 }"; // TaskQueryParams { id }\nlet b = 1;'
        m = mask(src)
        self.assertEqual(len(m), len(src))
        self.assertNotIn("TaskQueryParams", m)
        self.assertIn("let b = 1;", m)

    def test_raw_strings_chars_and_lifetimes(self):
        src = "let r = r#\"a \" TaskQueryParams {\"#; let c = '{'; fn f<'a>(x: &'a str) {}"
        m = mask(src)
        self.assertNotIn("TaskQueryParams", m)
        self.assertEqual(m.count("{"), 1)  # only the fn body brace survives, not the char literal's
        self.assertIn("'a", m)

    def test_nested_block_comments(self):
        m = mask("/* a /* b */ TaskQueryParams { */ x")
        self.assertNotIn("TaskQueryParams", m)

    def test_match_close_skips_nested(self):
        m = mask("{ a: foo({1}), b: [ (2) ] } tail")
        self.assertEqual(match_close(m, 0), m.index("}", m.index("]")))


class SplitTests(unittest.TestCase):
    def test_closure_commas_do_not_split(self):
        body = " id: x, tenant: Some(items.iter().map(|a, b| a + b).count().to_string()), history_length: None "
        got = split_fields(mask(body), body)
        self.assertEqual([k for k, _ in got], ["id", "tenant", "history_length"])

    def test_shorthand_and_base(self):
        body = " id, ..Default::default() "
        self.assertEqual(split_fields(mask(body), body), [("id", "id"), ("..", "Default::default()")])


class ConvertTests(unittest.TestCase):
    def test_required_only(self):
        r = run('let p = TaskQueryParams { id: "t".into(), tenant: None, history_length: None };')
        self.assertEqual(r.text, 'let p = TaskQueryParams::new("t".into());')
        self.assertEqual(r.converted, 1)

    def test_some_is_peeled_and_none_dropped(self):
        r = run('TaskQueryParams { id: x, tenant: Some(t), history_length: Some(5) }')
        self.assertEqual(r.text, "TaskQueryParams::new(x).with_tenant(t).with_history_length(5)")

    def test_non_some_option_falls_back_to_assignment(self):
        r = run("TaskQueryParams { id: x, tenant: self.tenant.clone(), history_length: None }")
        self.assertEqual(r.text, "{ let mut built = TaskQueryParams::new(x); built.tenant = self.tenant.clone(); built }")

    def test_shorthand_option_falls_back(self):
        r = run("TaskQueryParams { id, tenant, history_length: None }")
        self.assertEqual(r.text, "{ let mut built = TaskQueryParams::new(id); built.tenant = tenant; built }")

    def test_collection_field_and_empty_collection(self):
        r = run("AuthorizationCodeFlow { authorization_url: a, token_url: t, refresh_url: None, scopes: HashMap::new(), pkce_required: None }")
        self.assertEqual(r.text, "AuthorizationCodeFlow::new(a, t)")
        r = run("AuthorizationCodeFlow { authorization_url: a, token_url: t, refresh_url: Some(r), scopes: s, pkce_required: Some(true) }")
        self.assertEqual(r.text, "AuthorizationCodeFlow::new(a, t).with_refresh_url(r).with_scopes(s).with_pkce_required(true)")

    def test_path_prefix_is_kept(self):
        r = run("a2a_protocol_types::params::TaskQueryParams { id: x, tenant: None, history_length: None }")
        self.assertEqual(r.text, "a2a_protocol_types::params::TaskQueryParams::new(x)")

    def test_struct_update_syntax(self):
        r = run("MessageSendParams { message: m, ..base }")
        self.assertEqual(r.text, "{ let mut built = base; built.message = m; built }")

    def test_definitions_and_impls_are_untouched(self):
        for src in ("pub struct TaskQueryParams { pub id: String }",
                    "impl TaskQueryParams { fn f() {} }",
                    "impl Trait for TaskQueryParams { fn f() {} }",
                    "fn make() -> TaskQueryParams { build() }"):
            self.assertEqual(run(src).text, src, src)

    def test_strings_and_comments_are_untouched(self):
        src = 'let s = "TaskQueryParams { id: 1 }"; // TaskQueryParams { id: 1 }'
        self.assertEqual(run(src).text, src)

    def test_patterns_gain_a_rest_and_are_not_rewritten(self):
        r = run("let TaskQueryParams { id, tenant } = p;")
        self.assertEqual(r.text, "let TaskQueryParams { id, tenant, .. } = p;")
        self.assertEqual(r.patterns, 1)
        r = run("match p { TaskQueryParams { id, .. } => id, }")
        self.assertEqual(r.text, "match p { TaskQueryParams { id, .. } => id, }")
        r = run("if let TaskQueryParams { id } = p { }")
        self.assertEqual(r.text, "if let TaskQueryParams { id, .. } = p { }")

    def test_nested_literals_both_convert(self):
        r = run("MessageSendParams { message: m, tenant: None, configuration: Some(TaskQueryParams { id: x, tenant: None, history_length: None }), metadata: None }")
        self.assertEqual(r.text, "MessageSendParams::new(m).with_configuration(TaskQueryParams::new(x))")

    def test_comment_inside_a_literal_is_reported_not_rewritten(self):
        src = "TaskQueryParams { id: x, // the id\n tenant: None, history_length: None }"
        r = run(src)
        self.assertEqual(r.text, src)
        self.assertTrue(r.skipped and "comment" in r.skipped[0])

    def test_a_url_in_a_string_is_not_a_comment(self):
        r = run('TaskQueryParams { id: "https://x.test/a".into(), tenant: None, history_length: None }')
        self.assertEqual(r.text, 'TaskQueryParams::new("https://x.test/a".into())')
        self.assertEqual(r.skipped, [])

    def test_unknown_field_and_missing_required_are_reported(self):
        self.assertTrue(run("TaskQueryParams { id: x, nope: 1 }").skipped)
        self.assertTrue(run("TaskQueryParams { tenant: None, history_length: None }").skipped)

    def test_two_literals_in_one_file_apply_from_the_end(self):
        r = run("let a = TaskQueryParams { id: x, tenant: None, history_length: None };\nlet b = TaskQueryParams { id: y, tenant: Some(t), history_length: None };")
        self.assertEqual(r.text, "let a = TaskQueryParams::new(x);\nlet b = TaskQueryParams::new(y).with_tenant(t);")

    def test_a_different_type_with_a_longer_name_is_not_matched(self):
        src = "MyTaskQueryParams { id: x }"
        self.assertEqual(run(src).text, src)

    def test_string_list_literal(self):
        self.assertEqual(run('StringList { list: vec!["a".into()] }').text, 'StringList::new(vec!["a".into()])')


class ProtoTests(unittest.TestCase):
    def test_prost_types_with_the_same_name_are_left_alone(self):
        for src in ("pb::TaskIdParams { id: x, ..Default::default() }",
                    "a2a_protocol_types::proto::TaskIdParams { id: x }"):
            self.assertEqual(run(src).text, src, src)
        self.assertEqual(run("a2a_protocol_types::params::TaskIdParams { id: x, tenant: None }").text,
                         "a2a_protocol_types::params::TaskIdParams::new(x)")


class IntoTests(unittest.TestCase):
    def test_into_is_dropped_for_string_parameters_only(self):
        from specs import SPECS as S
        r = convert('TaskIdParams { id: "t".into(), tenant: Some("acme".into()) }', S)
        self.assertEqual(r.text, 'TaskIdParams::new("t").with_tenant("acme")')
        r = convert("ListTasksParams { page_size: Some(n.into()), tenant: Some(format!(\"{}\", a).into()), ..Default::default() }", S)
        self.assertEqual(r.text, 'ListTasksParams::new().with_tenant(format!("{}", a)).with_page_size(n.into())')

    def test_a_method_chain_ending_in_into_loses_only_the_into(self):
        from specs import SPECS as S
        self.assertEqual(convert("TaskIdParams { id: a.b().c.into(), tenant: None }", S).text, "TaskIdParams::new(a.b().c)")


class WrapTests(unittest.TestCase):
    def test_long_chain_breaks_at_top_level_with_calls_only(self):
        src = "    let p = TaskQueryParams { id: some_function(a, b), tenant: Some(format!(\"{}-{}\", x, y)), history_length: Some(10) };"
        r = convert(src, SPECS, wrap=60)
        self.assertEqual(r.text, "    let p = TaskQueryParams::new(some_function(a, b))\n        .with_tenant(format!(\"{}-{}\", x, y))\n        .with_history_length(10);")

    def test_short_chain_stays_on_one_line(self):
        src = "let p = TaskQueryParams { id: x, tenant: Some(t), history_length: None };"
        self.assertEqual(convert(src, SPECS, wrap=100).text, "let p = TaskQueryParams::new(x).with_tenant(t);")


if __name__ == "__main__":
    unittest.main()
