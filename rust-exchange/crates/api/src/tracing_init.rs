// P1-OPS-2 minimal scope: configure the global `tracing` subscriber.
//
// Two build modes:
//
//   default          — JSON formatter only (same as the original
//                      `tracing_subscriber::fmt()...` setup in main).
//                      Zero new runtime deps; behaviour unchanged.
//
//   --features otel  — JSON formatter + OpenTelemetry layer. The layer
//                      exports spans to an OTLP/HTTP endpoint when
//                      `OTEL_EXPORTER_OTLP_ENDPOINT` is set; when
//                      unset, the SDK builder still completes and the
//                      layer becomes a no-op exporter (spans flow
//                      through tracing as today).
//
// Env-var contract (only consulted when the `otel` feature is on):
//
//   OTEL_EXPORTER_OTLP_ENDPOINT
//       Collector URL, e.g. `http://otel-collector.observability:4318`.
//       If unset → no exporter; the layer is a sink.
//
//   OTEL_SERVICE_NAME
//       Resource attribute. Default: `rust-exchange-api`.
//
//   OTEL_EXPORTER_OTLP_HEADERS
//       Standard OTel env var, honoured natively by `opentelemetry_sdk`
//       (e.g. for SigNoz / Honeycomb API keys).
//
// Shutdown: callers MUST invoke `shutdown_telemetry()` before exit so
// in-flight spans flush to the collector. main does this in the
// signal-handling shutdown path.
//
// What this commit does NOT do:
//   - Inject `traceparent` into outbound wallet ChainAdapter HTTP calls
//     (chain RPC propagation). That requires per-call header injection
//     and is the next P1-OPS-2 milestone.
//   - Wire OTel into the worker tasks beyond what `tracing::Span::
//     current()` already gives them (tokio::spawn captures the parent
//     span by default — enough for the same-process worker chain).

use tracing_subscriber::EnvFilter;

#[cfg(feature = "otel")]
mod otel {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::{runtime, trace::TracerProvider, Resource};
    use opentelemetry::KeyValue;

    pub(super) struct OtelGuard {
        pub provider: TracerProvider,
    }

    pub(super) fn init_provider() -> Option<(OtelGuard, opentelemetry_sdk::trace::Tracer)> {
        let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok()?;
        let service_name = std::env::var("OTEL_SERVICE_NAME")
            .unwrap_or_else(|_| "rust-exchange-api".into());

        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(endpoint)
            .build()
            .ok()?;

        let provider = TracerProvider::builder()
            .with_batch_exporter(exporter, runtime::Tokio)
            .with_resource(Resource::new(vec![KeyValue::new(
                "service.name",
                service_name.clone(),
            )]))
            .build();
        let tracer = provider.tracer(service_name);
        Some((OtelGuard { provider }, tracer))
    }
}

#[cfg(not(feature = "otel"))]
pub fn init() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}

#[cfg(feature = "otel")]
pub fn init() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer().json();

    if let Some((guard, tracer)) = otel::init_provider() {
        // Stash guard in a OnceLock so shutdown_telemetry() can drain.
        let _ = OTEL_GUARD.set(guard);
        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(otel_layer)
            .init();
        tracing::info!("OpenTelemetry layer active");
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .init();
        tracing::info!(
            "OTEL_EXPORTER_OTLP_ENDPOINT not set; OTel layer skipped"
        );
    }
}

#[cfg(feature = "otel")]
static OTEL_GUARD: std::sync::OnceLock<otel::OtelGuard> = std::sync::OnceLock::new();

/// Flush in-flight spans to the collector. Call from the shutdown path
/// before the process exits — without this, the last batch of spans is
/// lost. No-op when `otel` feature is off or no endpoint was configured.
pub fn shutdown_telemetry() {
    #[cfg(feature = "otel")]
    {
        // We take a clone of the provider through the guard handle.
        // The guard itself can't be moved out of OnceLock, but provider
        // implements clone-of-handle via Arc internally.
        if let Some(guard) = OTEL_GUARD.get() {
            let _ = guard.provider.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_is_safe_when_uninitialised() {
        // Calling shutdown_telemetry() before init() must not panic.
        // This is the path a unit test takes when it does not need
        // tracing — the test framework hasn't called init().
        shutdown_telemetry();
    }

    #[test]
    fn env_filter_falls_back_to_info() {
        // Sanity check that the EnvFilter we'd hand to the subscriber
        // is constructible with no env. This guards against subscriber
        // init dying at startup if `RUST_LOG` happens to be unset.
        std::env::remove_var("RUST_LOG");
        let _ = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    }
}
