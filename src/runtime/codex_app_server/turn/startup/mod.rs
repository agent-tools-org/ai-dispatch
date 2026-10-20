// Startup RPCs and safe fallback eligibility before a turn has been submitted.
// Resume excludes replayed turns while preserving the existing thread identity.

use super::*;

impl Turn<'_> {
    pub fn start(&mut self, mut launch: Launch) -> Result<()> {
        self.request(
            "initialize",
            json!({"clientInfo":{"name":"aid","version":env!("CARGO_PKG_VERSION")}}),
        )?;
        self.rpc.send(json!({"method":"initialized"}))?;
        if launch.resume {
            launch.thread["excludeTurns"] = json!(true);
        }
        let response = self.request(
            if launch.resume {
                "thread/resume"
            } else {
                "thread/start"
            },
            launch.thread,
        )?;
        self.thread = response["thread"]["id"]
            .as_str()
            .context("Missing Codex thread id")?
            .to_string();
        crate::runtime::read_only_launch::record(
            self.sink.run.store, self.sink.run.task, self.sink.run.command, "app-server",
        )?;
        self.events.model = response["model"].as_str().map(str::to_string);
        self.sink.event(
            json!({"type":"thread.started","thread_id":self.thread,"model":self.events.model}),
        )?;
        let mut params =
            json!({"threadId":self.thread,"input":[{"type":"text","text":launch.prompt}]});
        if let Some(effort) = launch.effort {
            params["effort"] = effort;
        }
        self.start_requested = true;
        let response = self.request("turn/start", params)?;
        self.turn = Some(
            response["turn"]["id"]
                .as_str()
                .context("Missing Codex turn id")?
                .to_string(),
        );
        Ok(())
    }

    pub fn fallback_safe(&self) -> bool {
        !self.start_requested && self.turn.is_none()
    }
}

#[cfg(test)]
#[path = "startup_tests.rs"]
mod tests;
