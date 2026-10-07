use super::*;

#[derive(Debug)]
pub(in crate::web) struct ApiError {
    pub(in crate::web) status: StatusCode,
    pub(in crate::web) error: anyhow::Error,
}

impl ApiError {
    pub(in crate::web) fn bad(error: impl Into<anyhow::Error>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: error.into(),
        }
    }
    pub(in crate::web) fn conflict(message: &'static str) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            error: anyhow!(message),
        }
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(error: E) -> Self {
        Self::bad(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, self.error.to_string()).into_response()
    }
}

pub(in crate::web) type ApiResult<T> = std::result::Result<T, ApiError>;
