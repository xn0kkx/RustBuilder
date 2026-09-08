use crate::agent::build_agent;
use crate::config::BLOCKED_COUNTRIES;
use crate::ipapi::IpApiResponse;
use crate::protection::Protection;

impl Protection {
    pub(crate) fn check_ip(&self) {
        let agent = build_agent(self.timeout_secs);
        let response = match agent.get(&self.ip_api_url).call() {
            Ok(r)  => r,
            Err(_) => return,
        };
        let data: IpApiResponse = match response.into_json() {
            Ok(d)  => d,
            Err(_) => return,
        };
        if data.status != "success" {
            return;
        }
        if let Some(code) = data.country_code {
            if BLOCKED_COUNTRIES.contains(&code.as_str()) {
                self.on_fail();
            }
        }
    }
}
