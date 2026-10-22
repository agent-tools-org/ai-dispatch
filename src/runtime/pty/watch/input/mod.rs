// PTY input helpers moved from the shared monitor.
// Deps: parent monitor state, agent parsing, Store and PTY control.

use super::*;

impl MonitorState {
    pub(super) fn maybe_forward_input(
        &mut self,
        bridge: &mut PtyBridge,
        store: &Arc<Store>,
        task_id: &TaskId,
    ) -> Result<()> {
        if !self.awaiting_input {
            return Ok(());
        }
        let Some(input) = input_signal::take_response(task_id.as_str())? else {
            return Ok(());
        };
        if !Self::write_input_or_record_failure(bridge, store, task_id, "Response", &input, None)? {
            return Ok(());
        }
        store.insert_event(&TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Reasoning,
            detail: "Response written to PTY; consumption unconfirmed".to_string(),
            metadata: Some(json!({ "input_delivery": "written", "consumption": "unconfirmed" })),
        })?;
        register_inbound_echo(&mut self.inbound_echo_suppress, input);
        self.finish_input_delivery(store, task_id)?;
        Ok(())
    }

    pub(super) fn maybe_forward_steer(
        &mut self,
        bridge: &mut PtyBridge,
        store: &Arc<Store>,
        task_id: &TaskId,
    ) -> Result<()> {
        let Some(message) = input_signal::take_steer(task_id.as_str())? else {
            return Ok(());
        };
        if !Self::write_input_or_record_failure(bridge, store, task_id, "Steer", &message, None)? {
            return Ok(());
        }
        register_inbound_echo(&mut self.inbound_echo_suppress, message.clone());
        store.mark_delivered_matching_inbound(task_id.as_str(), &message)?;
        self.finish_input_delivery(store, task_id)?;
        store.insert_event(&TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Reasoning,
            detail: format!(
                "Steer written to PTY; consumption unconfirmed: {}",
                message.chars().take(200).collect::<String>()
            ),
            metadata: Some(json!({
                "input_delivery": "written", "source": "steer", "consumption": "unconfirmed"
            })),
        })?;
        Ok(())
    }

    pub(super) fn maybe_consume_reply(
        &mut self,
        bridge: &mut PtyBridge,
        store: &Arc<Store>,
        task_id: &TaskId,
    ) -> Result<()> {
        for message in store.pending_inbound_for_task(task_id.as_str())? {
            if self.failed_inbound_message_ids.contains(&message.id) {
                continue;
            }
            if !Self::write_input_or_record_failure(
                bridge,
                store,
                task_id,
                "Reply",
                &message.content,
                Some(message.id),
            )? {
                self.failed_inbound_message_ids.insert(message.id);
                break;
            }
            register_inbound_echo(&mut self.inbound_echo_suppress, message.content.clone());
            store.mark_delivered(message.id)?;
            self.finish_input_delivery(store, task_id)?;
            store.insert_event(&TaskEvent {
                task_id: task_id.clone(),
                timestamp: Local::now(),
                event_kind: EventKind::Reasoning,
                detail: format!(
                    "Reply written to PTY; consumption unconfirmed: {}",
                    message.content.chars().take(200).collect::<String>()
                ),
                metadata: Some(json!({
                    "message_id": message.id,
                    "source": message.source,
                    "input_delivery": "written",
                    "consumption": "unconfirmed",
                })),
            })?;
        }
        Ok(())
    }

    pub(super) fn write_input_or_record_failure(
        bridge: &mut PtyBridge,
        store: &Arc<Store>,
        task_id: &TaskId,
        source: &str,
        message: &str,
        message_id: Option<i64>,
    ) -> Result<bool> {
        let delivery = if bridge.is_alive() {
            bridge.write_input(message)
        } else {
            Err(anyhow::anyhow!("PTY child has already exited"))
        };
        let Err(error) = delivery else {
            return Ok(true);
        };
        store.insert_event(&TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Milestone,
            detail: format!(
                "{source} PTY write failed; consumption unconfirmed: {} — {error:#}",
                message.chars().take(200).collect::<String>()
            ),
            metadata: Some(json!({
                "input_delivery": "failed",
                "source": source,
                "message_id": message_id,
                "error": error.to_string(),
            })),
        })?;
        Ok(false)
    }

    pub(super) fn finish_input_delivery(
        &mut self,
        store: &Arc<Store>,
        task_id: &TaskId,
    ) -> Result<()> {
        if !self.awaiting_input {
            return Ok(());
        }
        crate::task::lifecycle::mark_running(store.as_ref(), task_id)?;
        self.awaiting_input = false;
        self.prompt_detector.reset_after_input();
        Ok(())
    }
}

impl MonitorState {
    pub(super) fn queue_idle_nudge(
        &mut self,
        store: &Arc<Store>,
        task_id: &TaskId,
        message: String,
    ) -> Result<()> {
        crate::task::unstick::queue_auto_nudge(store.as_ref(), task_id.as_str(), &message)?;
        self.idle_nudged = true;
        self.idle_warned = true;
        store.insert_event(&TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Reasoning,
            detail: "Auto-nudge queued".to_string(),
            metadata: Some(json!({
                "message": message, "source": "unstick-auto", "input_delivery": "queued"
            })),
        })?;
        Ok(())
    }
}
