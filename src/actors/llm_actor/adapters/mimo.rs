use genai::ServiceTarget;
use genai::resolver::Endpoint;

pub fn build_mimo_client(builder: genai::ClientBuilder) -> genai::Client {
    let mimo_use_token_plan = std::env::var("MIMO_USE_TOKEN_PLAN").unwrap_or_default() == "1";
    builder
        .with_service_target_resolver_fn(move |mut target: ServiceTarget| {
            if mimo_use_token_plan {
                target.endpoint = Endpoint::from_static("https://token-plan-cn.xiaomimimo.com/v1/");
            }
            Ok(target)
        })
        .build()
}
