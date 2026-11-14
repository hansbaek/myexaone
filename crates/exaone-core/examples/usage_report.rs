//! 사용 기록을 저장소로 옮기고 최근 하루 집계를 출력한다 (앱의 사용량 화면과 같은 경로).
//!
//! cargo run -p exaone-core --example usage_report

fn main() -> anyhow::Result<()> {
    let store = exaone_core::store::Store::open_default()?;
    let log = exaone_core::usage::log_file()?;
    let n = exaone_core::usage::ingest(&store, &log)?;
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs_f64()
        - 86400.0;
    let s = exaone_core::usage::summary(&store, since)?;
    println!("새로 옮긴 기록 {n}줄 ({})", log.display());
    println!("{}", serde_json::to_string_pretty(&s)?);
    Ok(())
}
