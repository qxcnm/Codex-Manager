//! Recover text that exists in a successful terminal snapshot but lacks the
//! output-item event consumed by Codex. Ordinary provider frames stay untouched.
use serde::Serialize;
use serde_json::Value;
use std::io::{self, Write};

const MAX_TRACKED_ITEMS: usize = 1024;
const MAX_TRACKED_ID_BYTES: usize = 64 * 1024;
const MAX_REPAIR_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub(super) struct ResponsesOutputRepair {
    observations: Vec<ItemObservation>,
    tracked_id_bytes: usize,
    repaired_bytes: usize,
    disabled: bool,
    warned_output_limit: bool,
}

struct ItemObservation {
    id: Option<String>,
    index: Option<u64>,
    done: bool,
    conflicted: bool,
}

impl ItemObservation {
    fn matches_item(&self, id: &str, index: u64) -> bool {
        self.id.as_deref() == Some(id) || self.index == Some(index)
    }

    fn conflicts_with(&self, id: Option<&str>, index: Option<u64>) -> bool {
        let conflicting_index = id.is_some()
            && self.id.as_deref() == id
            && self.index.is_some()
            && index.is_some()
            && self.index != index;
        let conflicting_id = index.is_some()
            && self.index == index
            && self.id.is_some()
            && id.is_some()
            && self.id.as_deref() != id;
        conflicting_index || conflicting_id
    }
}

impl ResponsesOutputRepair {
    pub(super) fn process(&mut self, value: &Value, successful_completion: bool) -> Vec<u8> {
        if self.disabled {
            return Vec::new();
        }
        let event_type = value.get("type").and_then(Value::as_str);
        if matches!(
            event_type,
            Some(
                "response.output_item.added"
                    | "response.output_item.done"
                    | "response.output_text.delta"
                    | "response.output_text.done"
                    | "response.content_part.added"
                    | "response.content_part.delta"
                    | "response.content_part.done"
            )
        ) {
            self.observe_event(value, event_type == Some("response.output_item.done"));
            return Vec::new();
        }
        if event_type != Some("response.completed") || !successful_completion {
            return Vec::new();
        }
        let Some(response) = value
            .get("response")
            .filter(|response| response.is_object())
        else {
            return Vec::new();
        };
        if response.get("status").and_then(Value::as_str) != Some("completed")
            || has_error(value)
            || has_error(response)
        {
            return Vec::new();
        }
        let Some(output) = response.get("output").and_then(Value::as_array) else {
            return Vec::new();
        };
        if output.len() > MAX_TRACKED_ITEMS {
            self.disable_tracking();
            return Vec::new();
        }

        // Validate all final identities before emitting anything. In particular,
        // duplicate IDs in the snapshot must not repair their first occurrence.
        for (index, item) in output.iter().enumerate() {
            if let Some(id) = item_id(item.get("id")) {
                self.observe(Some(id), Some(index as u64), false);
                if self.disabled {
                    return Vec::new();
                }
            }
        }

        let mut writer = CappedWrite::new(MAX_REPAIR_BYTES.saturating_sub(self.repaired_bytes));
        let mut repaired = Vec::new();
        for (index, item) in output.iter().enumerate() {
            let Some(id) = repairable_message_id(item) else {
                continue;
            };
            if self.observations.iter().any(|observation| {
                observation.matches_item(id, index as u64)
                    && (observation.done || observation.conflicted)
            }) {
                continue;
            }
            let event = OutputItemDoneEvent {
                event_type: "response.output_item.done",
                output_index: index,
                item,
            };
            if writer
                .write_all(b"event: response.output_item.done\ndata: ")
                .and_then(|()| serde_json::to_writer(&mut writer, &event).map_err(io::Error::other))
                .and_then(|()| writer.write_all(b"\n\n"))
                .is_err()
            {
                if !self.warned_output_limit {
                    log::warn!("event=responses_output_repair_skipped reason=output_limit");
                    self.warned_output_limit = true;
                }
                // Return none of this batch; a partial event could corrupt the
                // provider stream, and partial repairs could duplicate a retry.
                return Vec::new();
            }
            repaired.push((id, index as u64));
        }
        if repaired.is_empty() {
            return Vec::new();
        }
        self.repaired_bytes = self.repaired_bytes.saturating_add(writer.bytes.len());
        for (id, index) in &repaired {
            self.observe(Some(*id), Some(*index), true);
        }
        log::debug!(
            "event=responses_output_repair items={} bytes={}",
            repaired.len(),
            writer.bytes.len()
        );
        writer.bytes
    }

    fn observe_event(&mut self, value: &Value, done: bool) {
        let nested_id = item_id(value.pointer("/item/id"));
        let direct_id = item_id(value.get("item_id"));
        let index = value.get("output_index").and_then(Value::as_u64);
        self.observe(nested_id.or(direct_id), index, done);
        if nested_id.is_some() && direct_id.is_some() && nested_id != direct_id {
            self.observe(direct_id, index, done);
            // Even without an index, two identities on one event are ambiguous.
            for observation in &mut self.observations {
                if observation.id.as_deref() == nested_id || observation.id.as_deref() == direct_id
                {
                    observation.conflicted = true;
                }
            }
        }
    }

    fn observe(&mut self, id: Option<&str>, index: Option<u64>, done: bool) {
        if self.disabled || (id.is_none() && index.is_none()) {
            return;
        }
        if let Some(observation) = self
            .observations
            .iter_mut()
            .find(|observation| observation.id.as_deref() == id && observation.index == index)
        {
            observation.done |= done;
            return;
        }
        let id_bytes = id.map_or(0, str::len);
        if self.observations.len() >= MAX_TRACKED_ITEMS
            || id_bytes > MAX_TRACKED_ID_BYTES.saturating_sub(self.tracked_id_bytes)
        {
            self.disable_tracking();
            return;
        }
        let mut conflicted = false;
        for observation in &mut self.observations {
            if observation.conflicts_with(id, index) {
                observation.conflicted = true;
                conflicted = true;
            }
        }
        self.tracked_id_bytes += id_bytes;
        self.observations.push(ItemObservation {
            id: id.map(str::to_owned),
            index,
            done,
            conflicted,
        });
    }

    fn disable_tracking(&mut self) {
        self.disabled = true;
        self.observations.clear();
        self.tracked_id_bytes = 0;
        log::warn!("event=responses_output_repair_skipped reason=tracking_limit");
    }
}

fn item_id(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
}

fn has_error(value: &Value) -> bool {
    value.get("error").is_some_and(|error| !error.is_null())
        || value
            .pointer("/status_details/error")
            .is_some_and(|error| !error.is_null())
}

fn repairable_message_id(item: &Value) -> Option<&str> {
    if item.get("type").and_then(Value::as_str) != Some("message")
        || item.get("role").and_then(Value::as_str) != Some("assistant")
        || item.get("status").and_then(Value::as_str) != Some("completed")
        || has_error(item)
    {
        return None;
    }
    let content = item.get("content").and_then(Value::as_array)?;
    if !content.iter().any(|part| {
        part.get("type").and_then(Value::as_str) == Some("output_text")
            && part
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty())
    }) {
        return None;
    }
    item_id(item.get("id"))
}

#[derive(Serialize)]
struct OutputItemDoneEvent<'a> {
    #[serde(rename = "type")]
    event_type: &'static str,
    output_index: usize,
    item: &'a Value,
}

struct CappedWrite {
    bytes: Vec<u8>,
    limit: usize,
}

impl CappedWrite {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
}

impl Write for CappedWrite {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("response output repair byte limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(id: &str, text: &str) -> Value {
        json!({
            "id": id,
            "type": "message",
            "role": "assistant",
            "status": "completed",
            "content": [{"type":"output_text", "text":text, "annotations":[]}]
        })
    }

    fn completed(output: Vec<Value>) -> Value {
        json!({
            "type":"response.completed",
            "sequence_number":5,
            "response":{"id":"resp_test", "status":"completed", "output":output}
        })
    }

    fn parse_repaired(bytes: &[u8]) -> Vec<Value> {
        std::str::from_utf8(bytes)
            .unwrap()
            .split("\n\n")
            .filter(|frame| !frame.is_empty())
            .map(|frame| {
                assert!(frame.starts_with("event: response.output_item.done\ndata: "));
                serde_json::from_str(frame.split_once("\ndata: ").unwrap().1).unwrap()
            })
            .collect()
    }

    #[test]
    fn terminal_snapshot_repairs_only_missing_text_items_with_original_indices() {
        let first = message("msg_first", "第一条");
        let second = message("msg_second", "second\nline");
        let tool = json!({"id":"fc_1", "type":"function_call", "status":"completed"});
        let image = json!({"id":"ig_1", "type":"image_generation_call", "result":"AAAA"});
        let terminal = completed(vec![tool, first.clone(), image, second.clone()]);
        let original = terminal.clone();
        let mut repair = ResponsesOutputRepair::default();
        let events = parse_repaired(&repair.process(&terminal, true));
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["output_index"], 1);
        assert_eq!(events[0]["item"], first);
        assert_eq!(events[1]["output_index"], 3);
        assert_eq!(events[1]["item"], second);
        assert!(events
            .iter()
            .all(|event| event.get("sequence_number").is_none()));
        assert_eq!(terminal, original, "provider terminal is never changed");
        assert!(
            repair.process(&terminal, true).is_empty(),
            "do not repair twice"
        );
    }

    #[test]
    fn ordinary_done_stream_and_already_streamed_delta_do_not_duplicate_items() {
        let item = message("msg_text", "hello");
        let terminal = completed(vec![item.clone()]);
        for include_done in [false, true] {
            let mut repair = ResponsesOutputRepair::default();
            assert!(repair
                .process(
                    &json!({"type":"response.output_item.added", "output_index":0, "item":item}),
                    false
                )
                .is_empty());
            assert!(repair.process(&json!({"type":"response.output_text.delta", "output_index":0, "item_id":"msg_text", "delta":"hello"}), false).is_empty());
            if include_done {
                assert!(repair
                    .process(
                        &json!({"type":"response.output_item.done", "output_index":0, "item":item}),
                        false
                    )
                    .is_empty());
            }
            let events = parse_repaired(&repair.process(&terminal, true));
            assert_eq!(events.len(), usize::from(!include_done));
            if let Some(event) = events.first() {
                assert_eq!(
                    event["item"], item,
                    "repair only the missing done, not deltas"
                );
            }
        }
    }

    #[test]
    fn matching_done_id_or_index_alone_suppresses_repair() {
        let terminal = completed(vec![message("msg_text", "hello")]);
        for done in [
            json!({"type":"response.output_item.done", "item":{"id":"msg_text"}}),
            json!({"type":"response.output_item.done", "output_index":0}),
            json!({"type":"response.output_item.done", "output_index":8, "item":{"id":"msg_text"}}),
            json!({"type":"response.output_item.done", "output_index":0, "item":{"id":"msg_other"}}),
        ] {
            let mut repair = ResponsesOutputRepair::default();
            repair.process(&done, false);
            assert!(repair.process(&terminal, true).is_empty());
        }
    }

    #[test]
    fn conflicting_identities_skip_affected_items_but_allow_independent_text() {
        for previous in [
            json!({"type":"response.output_item.added", "output_index":0, "item":{"id":"msg_wrong"}}),
            json!({"type":"response.output_text.delta", "output_index":9, "item_id":"msg_conflict", "delta":"x"}),
            json!({"type":"response.output_text.delta", "output_index":0, "item_id":"msg_wrong", "item":{"id":"msg_conflict"}}),
        ] {
            let mut repair = ResponsesOutputRepair::default();
            repair.process(&previous, false);
            let events = parse_repaired(&repair.process(
                &completed(vec![
                    message("msg_conflict", "hidden"),
                    message("msg_independent", "shown"),
                ]),
                true,
            ));
            assert_eq!(events.len(), 1);
            assert_eq!(events[0]["item"]["id"], "msg_independent");
            assert_eq!(events[0]["output_index"], 1);
        }
        let mut repair = ResponsesOutputRepair::default();
        assert!(repair
            .process(
                &completed(vec![
                    message("msg_duplicate", "one"),
                    message("msg_duplicate", "two")
                ]),
                true
            )
            .is_empty());
    }

    #[test]
    fn unsuccessful_or_noncanonical_terminals_are_never_repaired() {
        let terminal = completed(vec![message("msg_text", "hello")]);
        assert!(ResponsesOutputRepair::default()
            .process(&terminal, false)
            .is_empty());
        for kind in [
            "response.failed",
            "response.incomplete",
            "response.done",
            "error",
        ] {
            let mut terminal = terminal.clone();
            terminal["type"] = json!(kind);
            assert!(ResponsesOutputRepair::default()
                .process(&terminal, true)
                .is_empty());
        }
        for status in ["failed", "incomplete", "cancelled", "in_progress"] {
            let mut terminal = terminal.clone();
            terminal["response"]["status"] = json!(status);
            assert!(ResponsesOutputRepair::default()
                .process(&terminal, true)
                .is_empty());
        }
        let mut missing_status = terminal.clone();
        missing_status["response"]
            .as_object_mut()
            .unwrap()
            .remove("status");
        assert!(ResponsesOutputRepair::default()
            .process(&missing_status, true)
            .is_empty());
        for error_path in [
            "/error",
            "/response/error",
            "/response/status_details/error",
        ] {
            let mut terminal = terminal.clone();
            let target = match error_path {
                "/error" => &mut terminal["error"],
                "/response/error" => &mut terminal["response"]["error"],
                _ => &mut terminal["response"]["status_details"]["error"],
            };
            *target = json!({});
            assert!(ResponsesOutputRepair::default()
                .process(&terminal, true)
                .is_empty());
        }
        let mut null_errors = terminal.clone();
        null_errors["error"] = Value::Null;
        null_errors["response"]["error"] = Value::Null;
        null_errors["response"]["status_details"]["error"] = Value::Null;
        assert_eq!(
            parse_repaired(&ResponsesOutputRepair::default().process(&null_errors, true)).len(),
            1
        );
    }

    #[test]
    fn only_explicitly_completed_assistant_text_messages_are_eligible() {
        let item = message("msg_text", "hello");
        let mut variants = Vec::new();
        for (key, value) in [
            ("id", json!(" ")),
            ("type", json!("function_call")),
            ("role", json!("user")),
            ("status", json!("in_progress")),
            ("content", json!([{"type":"refusal", "refusal":"no"}])),
            ("content", json!([{"type":"output_text", "text":" \n "}])),
            ("content", json!([{"type":"output_text", "text":17}])),
            ("error", json!({"message":"failed"})),
        ] {
            let mut variant = item.clone();
            variant[key] = value;
            variants.push(variant);
        }
        let mut missing_status = item.clone();
        missing_status.as_object_mut().unwrap().remove("status");
        variants.push(missing_status);
        for variant in variants {
            assert!(ResponsesOutputRepair::default()
                .process(&completed(vec![variant]), true)
                .is_empty());
        }
    }

    #[test]
    fn tracking_limits_disable_the_entire_request_without_eviction() {
        let mut repair = ResponsesOutputRepair::default();
        for index in 0..=MAX_TRACKED_ITEMS {
            repair.process(
                &json!({"type":"response.output_item.done", "output_index":index}),
                false,
            );
        }
        assert!(repair.disabled);
        assert!(repair.observations.is_empty());
        assert!(repair
            .process(&completed(vec![message("msg_new", "hello")]), true)
            .is_empty());

        let mut repair = ResponsesOutputRepair::default();
        repair.process(&json!({"type":"response.output_text.delta", "item_id":"x".repeat(MAX_TRACKED_ID_BYTES + 1), "output_index":0}), false);
        assert!(repair.disabled);
        assert!(repair
            .process(&completed(vec![message("msg_new", "hello")]), true)
            .is_empty());
    }

    #[test]
    fn repeated_events_reuse_tracking_and_large_snapshots_disable_before_output() {
        let mut repair = ResponsesOutputRepair::default();
        let delta = json!({"type":"response.output_text.delta", "item_id":"msg_text", "output_index":0, "delta":"x"});
        for _ in 0..MAX_TRACKED_ITEMS + 1 {
            repair.process(&delta, false);
        }
        assert!(!repair.disabled);
        assert_eq!(repair.observations.len(), 1);
        assert_eq!(repair.tracked_id_bytes, "msg_text".len());

        let output = (0..=MAX_TRACKED_ITEMS)
            .map(|index| message(&format!("msg_{index}"), "x"))
            .collect();
        assert!(repair.process(&completed(output), true).is_empty());
        assert!(repair.disabled);
    }

    #[test]
    fn oversized_batch_is_atomic_and_cumulative_output_is_bounded() {
        let mut repair = ResponsesOutputRepair::default();
        let first = message("msg_small", "small");
        let too_large = message("msg_large", &"x".repeat(MAX_REPAIR_BYTES));
        assert!(repair
            .process(&completed(vec![first.clone(), too_large]), true)
            .is_empty());
        assert_eq!(repair.repaired_bytes, 0);
        assert!(repair
            .observations
            .iter()
            .all(|observation| !observation.done));
        assert_eq!(
            parse_repaired(&repair.process(&completed(vec![first]), true)).len(),
            1
        );

        let mut repair = ResponsesOutputRepair {
            repaired_bytes: MAX_REPAIR_BYTES - 10,
            ..Default::default()
        };
        assert!(repair
            .process(&completed(vec![message("msg_last", "hello")]), true)
            .is_empty());
        assert_eq!(repair.repaired_bytes, MAX_REPAIR_BYTES - 10);
    }

    #[test]
    fn capped_writer_rejects_before_appending_past_its_limit() {
        let mut writer = CappedWrite::new(5);
        writer.write_all(b"1234").unwrap();
        assert!(writer.write_all(b"56").is_err());
        assert_eq!(writer.bytes, b"1234");
        writer.write_all(b"5").unwrap();
        assert_eq!(writer.bytes, b"12345");
    }
}
