use serde::Deserialize;

#[derive(Deserialize)]
pub struct IpApiResponse {
    pub status: String,
    #[serde(rename = "countryCode")]
    pub country_code: Option<String>,
}
