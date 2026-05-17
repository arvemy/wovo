use super::*;

#[test]
fn oauth_auto_fallback_is_limited_to_auth_class_errors() {
    assert!(oauth_error_allows_cli_fallback(&AppError::AuthNotFound));
    assert!(oauth_error_allows_cli_fallback(&AppError::MissingTokens));
    assert!(oauth_error_allows_cli_fallback(&AppError::TokenRefresh(
        "status 400: invalid_grant".to_string()
    )));
    assert!(oauth_error_allows_cli_fallback(&AppError::TokenRefresh(
        "status 401".to_string()
    )));
    assert!(oauth_error_allows_cli_fallback(&AppError::TokenRefresh(
        "status 403".to_string()
    )));
    assert!(oauth_error_allows_cli_fallback(&AppError::UsageFetch(
        "status 401".to_string()
    )));
    assert!(oauth_error_allows_cli_fallback(&AppError::UsageFetch(
        "status 403".to_string()
    )));
    assert!(!oauth_error_allows_cli_fallback(&AppError::UsageFetch(
        "status 429".to_string()
    )));
    assert!(!oauth_error_allows_cli_fallback(&AppError::UsageFetch(
        "decode failed".to_string()
    )));
    assert!(!oauth_error_allows_cli_fallback(&AppError::TokenRefresh(
        "connection reset".to_string()
    )));
}
