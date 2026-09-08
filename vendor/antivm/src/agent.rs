use std::time::Duration;

pub fn build_agent(timeout_secs: u64) -> ureq::Agent {
    let t = Duration::from_secs(timeout_secs);
    ureq::AgentBuilder::new()
        .timeout_connect(t)
        .timeout_read(t)
        .timeout_write(t)
        .build()
}
