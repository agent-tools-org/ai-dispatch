// Method-specific RPC deadlines and safe non-interactive server responses.
// Late steer acknowledgements remain unconfirmed rather than being labelled refusals.

use super::*;

impl Turn<'_> {
    pub(super) fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.rpc.request(method, params)?;
        let budget = match method {
            "initialize" | "thread/start" | "thread/resume" | "turn/start" => self.startup_deadline,
            "turn/interrupt" => Instant::now() + Duration::from_secs(3),
            _ => Instant::now() + super::super::control::STEER_WINDOW,
        };
        let deadline = budget.min(self.deadline);
        loop {
            if method != "turn/interrupt" && self.stopped()? {
                self.interrupt()?;
                return Err(control::Unconfirmed(format!(
                    "Codex {method} cancelled because the task is stopping; acceptance unconfirmed"
                ))
                .into());
            }
            if Instant::now() >= deadline {
                return Err(control::Unconfirmed(format!(
                    "Codex {method} response timed out; acceptance unconfirmed"
                ))
                .into());
            }
            let Some(message) = self.rpc.receive(Duration::from_millis(100))? else {
                continue;
            };
            if message["id"].as_u64() == Some(id) && message.get("method").is_none() {
                if let Some(error) = message.get("error") {
                    if method == "turn/start" && self.turn.is_none() {
                        self.start_requested = false;
                    }
                    bail!("Codex {method}: {error}");
                }
                return message
                    .get("result")
                    .cloned()
                    .context("Missing Codex RPC result");
            }
            self.notification(message)?;
        }
    }

    pub(super) fn interrupt(&mut self) -> Result<()> {
        if let Some(turn) = self.turn.clone() {
            self.request(
                "turn/interrupt",
                json!({"threadId":self.thread,"turnId":turn}),
            )?;
            let deadline = Instant::now() + Duration::from_secs(1);
            while !self.completed && Instant::now() < deadline {
                if let Some(message) = self.rpc.receive(Duration::from_millis(100))? {
                    self.notification(message)?;
                }
            }
        }
        self.sink.info.status = TaskStatus::Stopped;
        Ok(())
    }

    pub(super) fn server_request(&self, id: &Value, message: &Value) -> Result<()> {
        let result = match message["method"].as_str().unwrap_or_default() {
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                json!({"decision":"decline"})
            }
            "execCommandApproval" | "applyPatchApproval" => {
                json!({"decision":{"denied":{"rejection":"Non-interactive approval declined"}}})
            }
            "item/permissions/requestApproval" => json!({"permissions":{},"scope":"turn"}),
            "mcpServer/elicitation/request" => json!({"action":"cancel"}),
            "item/tool/requestUserInput" => json!({"answers":{}}),
            "item/tool/call" => json!({"success":false,"contentItems":[]}),
            method => {
                self.rpc.send(json!({"id":id,"error":{"code":-32601,
                    "message":format!("Unsupported non-interactive request: {method}")}}))?;
                bail!("Unsupported Codex server request: {method}");
            }
        };
        self.rpc.send(json!({"id":id,"result":result}))
    }
}
