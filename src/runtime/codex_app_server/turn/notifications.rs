// App-server notification validation and active-turn state updates.
// Filters other threads and preserves terminal status independently of RPC responses.

use super::*;

impl Turn<'_> {
    pub(super) fn notification(&mut self, message: Value) -> Result<()> {
        if let Some(id) = message.get("id") {
            if message.get("method").is_none() {
                return Ok(());
            }
            return self.server_request(id, &message);
        }
        if let Some(thread) = message["params"]["threadId"].as_str() {
            if !self.thread.is_empty() && thread != self.thread {
                return Ok(());
            }
        }
        if message["method"] == "thread/tokenUsage/updated"
            && message["params"]["turnId"].as_str() != self.turn.as_deref()
        {
            return Ok(());
        }
        if message["method"] == "turn/completed" {
            let turn = message["params"]["turn"]["id"]
                .as_str()
                .context("Missing completed turn id")?;
            if self
                .turn
                .as_deref()
                .is_some_and(|expected| expected != turn)
            {
                return Ok(());
            }
        }
        self.observe_progress(&message);
        match message["method"].as_str() {
            Some("turn/started") => {
                self.turn = message["params"]["turn"]["id"].as_str().map(str::to_string);
            }
            Some("turn/completed") => {
                self.completed = true;
                self.terminal = Some(message.clone());
            }
            _ => {}
        }
        if message["method"] == "turn/completed" {
            return Ok(());
        }
        for event in self.events.notification(&message) {
            self.sink.event(event)?;
        }
        Ok(())
    }
    fn observe_progress(&mut self, message: &Value) {
        let method = message["method"].as_str().unwrap_or_default();
        let item = &message["params"]["item"];
        if (method.starts_with("item/") && item["type"] != "userMessage")
            || method == "thread/tokenUsage/updated"
            || method == "error"
        {
            self.has_output = true;
            self.last_progress = std::time::SystemTime::now();
        }
    }
}
