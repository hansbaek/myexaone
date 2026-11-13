//! EXAONE Studio 핵심 로직. 명령줄 도구(`exaone`)와 데스크톱 앱이 함께 쓴다.
//!
//! - [`paths`], [`settings`]: 데이터 폴더와 설정
//! - [`secrets`]: API 키 (OS 키체인 / 자격 증명 관리자)
//! - [`runtime`]: uv로 LiteLLM 실행 환경 설치
//! - [`gateway`]: 게이트웨이 프로세스 실행과 상태 확인
//! - [`launch`]: Claude Code·Codex를 게이트웨이에 붙여 실행
//! - [`exaone_api`], [`mcp`]: EXAONE 직접 호출과 MCP 서버
//! - [`doctor`]: 환경 진단

pub mod assets;
pub mod doctor;
pub mod exaone_api;
pub mod gateway;
pub mod launch;
pub mod mcp;
pub mod paths;
pub mod process;
pub mod runtime;
pub mod secrets;
pub mod settings;

/// 게이트웨이에 등록된 모델 이름 (gateway/config.yaml 의 model_name).
pub const GATEWAY_MODEL: &str = "k-exaone";
/// EXAONE API의 실제 모델 ID.
pub const UPSTREAM_MODEL: &str = "k-exaone_v2.5";
