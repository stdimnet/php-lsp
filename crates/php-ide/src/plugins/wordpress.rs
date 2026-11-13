//! WordPress: completion and navigation for action / filter hook names.

use super::{string_args_of_calls, Cached, Plugin, PluginCx, StringArg, StringCandidate};
use crate::CompletionKind;

const FIRE_ACTION: &[&str] = &["do_action", "do_action_ref_array", "do_action_deprecated"];
const FIRE_FILTER: &[&str] = &["apply_filters", "apply_filters_ref_array", "apply_filters_deprecated"];
const USE_ACTION: &[&str] =
    &["add_action", "remove_action", "has_action", "did_action", "remove_all_actions", "doing_action"];
const USE_FILTER: &[&str] =
    &["add_filter", "remove_filter", "has_filter", "remove_all_filters", "doing_filter"];

#[derive(Default)]
pub struct WordPress {
    hooks: Cached<Vec<StringCandidate>>,
}

fn collect(cx: &PluginCx) -> Vec<StringCandidate> {
    let mut out: Vec<StringCandidate> = Vec::new();
    for (id, f) in cx.ws.files().filter(|(_, f)| f.kind != php_index::FileKind::Stub) {
        if !(f.text.contains("do_action") || f.text.contains("apply_filters")) {
            continue;
        }
        let Some(parse) = cx.ws.parse(id) else { continue };
        let root = parse.syntax_node();
        for (names, kind) in [(FIRE_ACTION, "action"), (FIRE_FILTER, "filter")] {
            for (hook, range) in string_args_of_calls(&root, names, 0) {
                if out.iter().any(|c| c.value == hook) {
                    continue;
                }
                let line = f.line_index.line_col(range.start()).line;
                out.push(StringCandidate {
                    value: hook,
                    detail: Some(kind.to_string()),
                    kind: CompletionKind::Constant,
                    location: Some((f.path.to_string(), line)),
                });
            }
        }
    }
    out.sort_by(|a, b| a.value.cmp(&b.value));
    out
}

impl Plugin for WordPress {
    fn name(&self) -> &'static str {
        "wordpress"
    }

    fn string_candidates(&self, cx: &PluginCx, arg: &StringArg) -> Vec<StringCandidate> {
        if arg.index != 0 {
            return Vec::new();
        }
        let wants = if USE_ACTION.iter().any(|n| arg.callee.is_function(n))
            || FIRE_ACTION.iter().any(|n| arg.callee.is_function(n))
        {
            "action"
        } else if USE_FILTER.iter().any(|n| arg.callee.is_function(n))
            || FIRE_FILTER.iter().any(|n| arg.callee.is_function(n))
        {
            "filter"
        } else {
            return Vec::new();
        };
        self.hooks
            .get(cx.ws.revision(), || collect(cx))
            .iter()
            .filter(|c| c.detail.as_deref() == Some(wants))
            .cloned()
            .collect()
    }
}
