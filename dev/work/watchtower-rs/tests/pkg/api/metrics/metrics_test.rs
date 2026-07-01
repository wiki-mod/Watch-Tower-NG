#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use watchtower_rs::api::{Api, HttpRequest, HttpResponse};
use watchtower_rs::api_metrics::{ApiMetrics, PATH};
use watchtower_rs::metrics::{self, Metric};

const TOKEN: &str = "123123123";

#[test]
fn should_serve_metrics() {
    let api = Api::new(TOKEN);
    let metrics_handler = ApiMetrics::legacy();
    let (path, handle) = metrics_handler.into_parts();
    let handle_req = api.require_token(move |_| HttpResponse::plain(200, handle()));

    assert_eq!(path, PATH);
    assert_metrics_match(
        &get_with_token(handle_req.as_ref(), path),
        &[("watchtower_containers_updated", "0")],
    );

    metrics::RegisterScan(Some(&Metric {
        scanned: 4,
        updated: 3,
        failed: 1,
    }));

    assert_metrics_match(
        &get_with_token(handle_req.as_ref(), path),
        &[
            ("watchtower_containers_updated", "3"),
            ("watchtower_containers_failed", "1"),
            ("watchtower_containers_scanned", "4"),
            ("watchtower_scans_total", "1"),
            ("watchtower_scans_skipped", "0"),
        ],
    );

    for _ in 0..3 {
        metrics::RegisterScan(None);
    }

    assert_metrics_match(
        &get_with_token(handle_req.as_ref(), path),
        &[
            ("watchtower_scans_total", "4"),
            ("watchtower_scans_skipped", "3"),
        ],
    );
}

fn get_with_token(
    handler: &dyn Fn(&HttpRequest) -> HttpResponse,
    path: &str,
) -> BTreeMap<String, String> {
    let mut metric_map = BTreeMap::new();

    let request = HttpRequest {
        method: "GET".to_string(),
        path: path.to_string(),
        headers: BTreeMap::from([(
            "Authorization".to_string(),
            format!("Bearer {TOKEN}"),
        )]),
        body: String::new(),
    };

    let response = handler(&request);

    for line in response.body.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
            metric_map.insert(name.to_string(), value.to_string());
        }
    }

    metric_map
}

fn assert_metrics_match(actual: &BTreeMap<String, String>, expected: &[(&str, &str)]) {
    for (name, value) in expected {
        assert_eq!(actual.get(*name), Some(&value.to_string()));
    }
}

