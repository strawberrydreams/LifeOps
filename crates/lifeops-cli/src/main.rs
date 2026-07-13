mod run;

use clap::{Parser, Subcommand};
use run::{run, Args};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "lifeops", about = "LifeOps CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// archives Markdown을 엔티티/노트로 임포트
    Import {
        /// 스캔 루트 디렉터리
        dir: PathBuf,
        /// 규칙 파일
        #[arg(long, default_value = "import-rules.yaml")]
        rules: PathBuf,
        /// 스키마 디렉터리
        #[arg(long, default_value = "schemas")]
        schemas: PathBuf,
        /// DB 경로
        #[arg(long, default_value = "data/lifeops.db")]
        db: PathBuf,
        /// 실제로 DB에 쓴다(미지정 시 dry-run)
        #[arg(long, conflicts_with = "dry_run")]
        commit: bool,
        /// 쓰기 없이 계획과 통계만 출력한다(기본 동작을 명시하는 호환 옵션)
        #[arg(long, conflicts_with = "commit")]
        dry_run: bool,
        /// 기존 $src가 있어도 소스값으로 갱신
        #[arg(long, requires = "commit")]
        force: bool,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Import {
            dir,
            rules,
            schemas,
            db,
            commit,
            dry_run,
            force,
        } => {
            let args = Args {
                dir,
                rules,
                schemas,
                db,
                commit,
                dry_run,
                force,
            };
            let dry = !args.commit;
            match run(args).await {
                Ok(report) => print_report(&report, dry),
                Err(error) => {
                    eprintln!("임포트 실패: {error}");
                    std::process::exit(1);
                }
            }
        }
    }
}

fn print_report(report: &lifeops_core::import::ImportReport, dry: bool) {
    let stats = &report.stats;
    println!("스캔 {}  파싱경고 {}", stats.scanned, stats.parse_warnings);
    let routed: Vec<String> = stats
        .routed
        .iter()
        .map(|(entity_type, count)| format!("{entity_type} {count}"))
        .collect();
    println!(
        "라우팅   {}  스킵(규칙) {}  드롭(필수) {}",
        routed.join("  "),
        stats.skipped_rule,
        stats.dropped_required
    );
    if stats.default_fallback > 0 {
        println!(
            "경고     △ {} files → default(노트)",
            stats.default_fallback
        );
    }
    if stats.price_warnings > 0 {
        println!(
            "경고     △ 가격 파싱 실패 {} (원문 메모 보존)",
            stats.price_warnings
        );
    }
    if dry {
        println!("(dry-run: 쓰기 없음. --commit 으로 반영)");
    } else {
        println!(
            "쓰기     ✓ created {}  updated {}  skipped(기존) {}",
            report.created, report.updated, report.skipped_existing
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import는_dry_run이_기본이고_명시_옵션도_지원한다() {
        let parsed = Cli::try_parse_from(["lifeops", "import", "archives"]).unwrap();
        let Command::Import {
            commit, dry_run, ..
        } = parsed.command;
        assert!(!commit && !dry_run);

        let parsed = Cli::try_parse_from(["lifeops", "import", "archives", "--dry-run"]).unwrap();
        let Command::Import {
            commit, dry_run, ..
        } = parsed.command;
        assert!(!commit && dry_run);
    }

    #[test]
    fn commit과_dry_run은_상호배타적이다() {
        let result =
            Cli::try_parse_from(["lifeops", "import", "archives", "--commit", "--dry-run"]);
        assert!(result.is_err());
    }

    #[test]
    fn force는_commit을_요구한다() {
        let without_commit = Cli::try_parse_from(["lifeops", "import", "archives", "--force"]);
        assert!(without_commit.is_err());

        let with_commit =
            Cli::try_parse_from(["lifeops", "import", "archives", "--commit", "--force"]);
        assert!(with_commit.is_ok());
    }
}
