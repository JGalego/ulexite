use ulx_ast::ArtifactType;

/// A capability's declared `accepts`/`produces` signature (§9.2, §9.6,
/// §15.1). The v0.1 runtime's stdlib capabilities are hardcoded here; a
/// real provider-plugin registry (§12.4) would populate this dynamically —
/// see `docs/spec/25-future-directions.md` RFC-7 for the conformance-suite
/// plan that would make this pluggable and certifiable.
#[derive(Debug, Clone)]
pub struct CapabilitySpec {
    pub name: &'static str,
    pub accepts: Vec<ArtifactType>,
    pub produces: Vec<ArtifactType>,
}

pub fn stdlib_capabilities() -> Vec<CapabilitySpec> {
    use ArtifactType::*;
    vec![
        CapabilitySpec {
            name: "chat",
            accepts: vec![Text, Markdown, Json, Image],
            produces: vec![Text, Json],
        },
        CapabilitySpec {
            name: "vision",
            accepts: vec![Image, Pdf, Video],
            produces: vec![Text, Json],
        },
        CapabilitySpec {
            name: "embed",
            accepts: vec![Text, Image],
            produces: vec![Embedding],
        },
        CapabilitySpec {
            name: "transcribe",
            accepts: vec![Audio, Video],
            produces: vec![Text],
        },
        CapabilitySpec {
            name: "speak",
            accepts: vec![Text],
            produces: vec![Audio],
        },
        CapabilitySpec {
            name: "generate_image",
            accepts: vec![Text],
            produces: vec![Image],
        },
    ]
}
