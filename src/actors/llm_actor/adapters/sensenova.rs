use genai::ServiceTarget;
use genai::adapter::AdapterKind;
use genai::resolver::Endpoint;

pub fn build_sensenova_client(builder: genai::ClientBuilder) -> genai::Client {
    builder
        .with_adapter_kind(AdapterKind::OpenAI)
        .with_service_target_resolver_fn(move |mut target: ServiceTarget| {
            target.endpoint = Endpoint::from_static("https://token.sensenova.cn/v1/");
            Ok(target)
        })
        .build()
}
