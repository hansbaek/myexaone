//! 실행 파일에 넣어 두는 파일들. 저장소의 원본을 그대로 쓴다 (원본은 하나만 유지).

/// LiteLLM 게이트웨이 설정.
pub const GATEWAY_CONFIG: &str = include_str!("../../../gateway/config.yaml");
/// 게이트웨이 파이썬 의존성.
pub const REQUIREMENTS: &str = include_str!("../../../requirements.txt");
/// 실제 EXAONE API 기능 점검 스크립트.
pub const VERIFY_API: &str = include_str!("../../../tests/verify_api.py");
/// 게이트웨이 사용 기록 콜백 (설정 파일과 같은 폴더에 둔다).
pub const USAGE_LOGGER: &str = include_str!("../../../gateway/usage_logger.py");
