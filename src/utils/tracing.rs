//! OpenTelemetry tracing and observability integration
//!
//! Provides distributed tracing and metrics export to OpenTelemetry collectors.

use opentelemetry::{
    global,
    sdk::{
        propagation::TraceContextPropagator,
        trace::{self, RandomIdGenerator, Sampler},
        Resource,
    },
    trace::{TraceError, Tracer},
    KeyValue,
};
use opentelemetry_otlp::WithExportConfig;
use std::time::Duration;
use tracing::{debug, info, Subscriber};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{layer::SubscriberExt, EnvFilter, Registry};

/// OpenTelemetry configuration
#[derive(Debug, Clone)]
pub struct OtelConfig {
    /// OTLP endpoint URL
    pub endpoint: String,
    /// Service name
    pub service_name: String,
    /// Service version
    pub service_version: String,
    /// Sampling ratio (0.0 to 1.0)
    pub sampling_ratio: f64,
    /// Export timeout
    pub export_timeout: Duration,
    /// Batch export config
    pub batch_config: BatchConfig,
}

impl Default for OtelConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:4317".to_string(),
            service_name: "super-mcp".to_string(),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            sampling_ratio: 1.0,
            export_timeout: Duration::from_secs(10),
            batch_config: BatchConfig::default(),
        }
    }
}

/// Batch export configuration
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Maximum queue size
    pub max_queue_size: usize,
    /// Scheduled delay for batch export
    pub scheduled_delay: Duration,
    /// Max export batch size
    pub max_export_batch_size: usize,
    /// Max export timeout
    pub max_export_timeout: Duration,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_queue_size: 2048,
            scheduled_delay: Duration::from_secs(5),
            max_export_batch_size: 512,
            max_export_timeout: Duration::from_secs(30),
        }
    }
}

/// Initialize OpenTelemetry tracing
pub fn init_tracer(config: &OtelConfig) -> Result<impl Tracer, TraceError> {
    global::set_text_map_propagator(TraceContextPropagator::new());

    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(&config.endpoint)
                .with_timeout(config.export_timeout),
        )
        .with_trace_config(
            trace::config()
                .with_sampler(Sampler::TraceIdRatioBased(config.sampling_ratio))
                .with_id_generator(RandomIdGenerator::default())
                .with_resource(Resource::new(vec![
                    KeyValue::new("service.name", config.service_name.clone()),
                    KeyValue::new("service.version", config.service_version.clone()),
                    KeyValue::new("deployment.environment", std::env::var("ENVIRONMENT").unwrap_or_else(|_| "production".to_string())),
                ])),
        )
        .with_batch_config(
            opentelemetry::sdk::trace::BatchConfig::default()
                .with_max_queue_size(config.batch_config.max_queue_size)
                .with_scheduled_delay(config.batch_config.scheduled_delay)
                .with_max_export_batch_size(config.batch_config.max_export_batch_size)
                .with_max_export_timeout(config.batch_config.max_export_timeout),
        )
        .install_batch(opentelemetry::runtime::Tokio)?;

    info!(
        "OpenTelemetry tracer initialized: endpoint={}, service={}",
        config.endpoint, config.service_name
    );

    Ok(tracer)
}

/// Initialize OpenTelemetry metrics
pub fn init_metrics(config: &OtelConfig) -> Result<opentelemetry::sdk::metrics::MeterProvider, TraceError> {
    let meter_provider = opentelemetry_otlp::new_pipeline()
        .metrics(opentelemetry::runtime::Tokio)
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(&config.endpoint)
                .with_timeout(config.export_timeout),
        )
        .with_resource(Resource::new(vec![
            KeyValue::new("service.name", config.service_name.clone()),
            KeyValue::new("service.version", config.service_version.clone()),
        ]))
        .build()?;

    global::set_meter_provider(meter_provider.clone());

    info!(
        "OpenTelemetry metrics initialized: endpoint={}",
        config.endpoint
    );

    Ok(meter_provider)
}

/// Create a tracing subscriber with OpenTelemetry support
pub fn create_subscriber_with_otel(
    otel_config: &OtelConfig,
) -> Result<impl Subscriber, TraceError> {
    let tracer = init_tracer(otel_config)?;

    let telemetry = OpenTelemetryLayer::new(tracer);

    let subscriber = Registry::default()
        .with(EnvFilter::from_default_env())
        .with(telemetry)
        .with(tracing_subscriber::fmt::layer());

    Ok(subscriber)
}

/// Initialize tracing with or without OpenTelemetry
pub fn init_tracing(otel_enabled: bool, otel_config: Option<&OtelConfig>) {
    if otel_enabled {
        if let Some(config) = otel_config {
            match create_subscriber_with_otel(config) {
                Ok(subscriber) => {
                    if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
                        eprintln!("Failed to set tracing subscriber: {}", e);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to initialize OpenTelemetry: {}", e);
                    init_fallback_tracing();
                }
            }
        } else {
            init_fallback_tracing();
        }
    } else {
        init_fallback_tracing();
    }
}

/// Initialize basic tracing without OpenTelemetry
fn init_fallback_tracing() {
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .finish();

    if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
        eprintln!("Failed to set fallback tracing subscriber: {}", e);
    }
}

/// Shutdown OpenTelemetry (flushes remaining spans)
pub fn shutdown_otel() {
    global::shutdown_tracer_provider();
    global::shutdown_meter_provider();
    info!("OpenTelemetry shut down");
}

/// Create a span with standard MCP attributes
#[macro_export]
macro_rules! mcp_span {
    ($name:expr, $method:expr, $server:expr) => {
        tracing::info_span!(
            $name,
            mcp.method = $method,
            mcp.server = $server,
            otel.kind = "server",
        )
    };
}

/// Trace context extraction from HTTP headers
pub fn extract_trace_context(headers: &axum::http::HeaderMap) -> opentelemetry::Context {
    use opentelemetry::propagation::TextMapPropagator;
    
    let extractor = HeaderMapExtractor(headers);
    global::get_text_map_propagator(|propagator| {
        propagator.extract(&extractor)
    })
}

/// Trace context injection into HTTP headers
pub fn inject_trace_context(headers: &mut axum::http::HeaderMap, context: &opentelemetry::Context) {
    use opentelemetry::propagation::TextMapPropagator;
    
    let mut injector = HeaderMapInjector(headers);
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(context, &mut injector);
    });
}

/// Helper struct to extract trace context from headers
struct HeaderMapExtractor<'a>(&'a axum::http::HeaderMap);

impl<'a> opentelemetry::propagation::Extractor for HeaderMapExtractor<'a> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}

/// Helper struct to inject trace context into headers
struct HeaderMapInjector<'a>(&'a mut axum::http::HeaderMap);

impl<'a> opentelemetry::propagation::Injector for HeaderMapInjector<'a> {
    fn set(&mut self, key: &str, value: String) {
        if let Ok(name) = axum::http::HeaderName::from_bytes(key.as_bytes()) {
            if let Ok(val) = axum::http::HeaderValue::from_str(&value) {
                self.0.insert(name, val);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_otel_config_default() {
        let config = OtelConfig::default();
        assert_eq!(config.endpoint, "http://localhost:4317");
        assert_eq!(config.service_name, "super-mcp");
        assert_eq!(config.sampling_ratio, 1.0);
    }

    #[test]
    fn test_batch_config_default() {
        let config = BatchConfig::default();
        assert_eq!(config.max_queue_size, 2048);
        assert_eq!(config.max_export_batch_size, 512);
    }

    #[tokio::test]
    async fn test_header_map_extractor() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("traceparent", "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".parse().unwrap());

        let extractor = HeaderMapExtractor(&headers);
        assert_eq!(
            extractor.get("traceparent"),
            Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01")
        );
    }

    #[tokio::test]
    async fn test_header_map_injector() {
        let mut headers = axum::http::HeaderMap::new();
        
        {
            let mut injector = HeaderMapInjector(&mut headers);
            injector.set("traceparent", "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".to_string());
        }

        assert!(headers.contains_key("traceparent"));
    }
}
