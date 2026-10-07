use super::*;

pub(in crate::web) async fn validate_origin(
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let Some(host) = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return (StatusCode::BAD_REQUEST, "Host header is required").into_response();
    };
    let host_name = host.rsplit_once(':').map_or(host, |(name, _)| name);
    if host_name != "localhost" && host_name.parse::<Ipv4Addr>().is_err() {
        return (
            StatusCode::BAD_REQUEST,
            "Host must be an IPv4 address or localhost",
        )
            .into_response();
    }
    if let Some(origin) = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        if origin != format!("http://{host}") {
            return (StatusCode::FORBIDDEN, "origin mismatch").into_response();
        }
    }
    next.run(request).await
}
