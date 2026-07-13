use lifeops_core::entity::EntityStore;
use lifeops_core::import::{
    apply, parse_document, plan, ApplyOpts, FileInput, ImportReport, RuleSet,
};
use lifeops_core::schema::SchemaSet;
use std::io;
use std::path::PathBuf;
use walkdir::WalkDir;

#[derive(Clone)]
pub struct Args {
    pub dir: PathBuf,
    pub rules: PathBuf,
    pub schemas: PathBuf,
    pub db: PathBuf,
    pub commit: bool,
    pub dry_run: bool,
    pub force: bool,
}

pub async fn run(args: Args) -> Result<ImportReport, Box<dyn std::error::Error>> {
    if args.commit && args.dry_run {
        return Err(invalid_input(
            "--commit과 --dry-run은 함께 사용할 수 없습니다",
        ));
    }
    if args.force && !args.commit {
        return Err(invalid_input("--force는 --commit과 함께 사용해야 합니다"));
    }

    let schemas = SchemaSet::load_dir(&args.schemas)?;
    let rules_source = std::fs::read_to_string(&args.rules)?;
    let rules = RuleSet::from_yaml(&rules_source)?;
    let mut files = Vec::new();
    for entry in WalkDir::new(&args.dir) {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type().is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("md")
        {
            continue;
        }
        let relative = path.strip_prefix(&args.dir).unwrap_or(path);
        let relpath = relative.to_string_lossy().replace('\\', "/");
        let content = std::fs::read_to_string(path)?;
        files.push(FileInput {
            relpath,
            doc: parse_document(&content),
        });
    }
    files.sort_by(|left, right| left.relpath.cmp(&right.relpath));

    let planned = plan(&rules, &schemas, &files);
    if !planned.stats.config_errors.is_empty() {
        return Err(invalid_input(format!(
            "규칙/schema 설정 오류: {}",
            planned.stats.config_errors.join(" / ")
        )));
    }
    if !args.commit {
        return Ok(ImportReport {
            stats: planned.stats.clone(),
            created: 0,
            updated: 0,
            skipped_existing: 0,
        });
    }

    let store = EntityStore::open(&args.db).await?;
    Ok(apply(&store, &schemas, &planned, ApplyOpts { force: args.force }).await?)
}

fn invalid_input(message: impl Into<String>) -> Box<dyn std::error::Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let schemas = dir.path().join("schemas");
        std::fs::create_dir(&schemas).unwrap();
        std::fs::write(
            schemas.join("노트.yaml"),
            "type: 노트\nfields:\n  제목: { kind: text, required: true }\n  본문: { kind: richtext }\n",
        )
        .unwrap();
        let rules = dir.path().join("rules.yaml");
        std::fs::write(
            &rules,
            "rules:\n  - default:\n      to: 노트\n      map: { 제목: fm.title | filename, 본문: body }\n      provenance: imported\n",
        )
        .unwrap();
        let archive = dir.path().join("archive");
        std::fs::create_dir_all(archive.join("sub")).unwrap();
        std::fs::write(archive.join("a.md"), "---\ntitle: 문서A\n---\n본문A").unwrap();
        std::fs::write(archive.join("sub/b.md"), "---\ntitle: 문서B\n---\n본문B").unwrap();
        std::fs::write(archive.join("skip.txt"), "md 아님").unwrap();
        let db = dir.path().join("test.db");
        (dir, schemas, rules, archive, db)
    }

    fn args(schemas: PathBuf, rules: PathBuf, dir: PathBuf, db: PathBuf) -> Args {
        Args {
            dir,
            rules,
            schemas,
            db,
            commit: false,
            dry_run: false,
            force: false,
        }
    }

    #[tokio::test]
    async fn dry_run은_db를_만들지_않고_commit은_멱등하게_쓴다() {
        let (_temp, schemas, rules, dir, db) = fixture();
        let base = args(schemas, rules, dir, db.clone());

        let report = run(base.clone()).await.unwrap();
        assert_eq!(report.stats.scanned, 2);
        assert_eq!(report.stats.routed.get("노트"), Some(&2));
        assert_eq!((report.created, report.updated), (0, 0));
        assert!(!db.exists(), "dry-run은 DB 파일도 만들지 않는다");

        let first = run(Args {
            commit: true,
            ..base.clone()
        })
        .await
        .unwrap();
        assert_eq!(first.created, 2);
        assert_eq!(store_count(&db).await, 2);

        let second = run(Args {
            commit: true,
            ..base
        })
        .await
        .unwrap();
        assert_eq!((second.created, second.skipped_existing), (0, 2));
    }

    #[tokio::test]
    async fn 잘못된_규칙_schema_조합은_dry_run에서도_실패한다() {
        let (_temp, schemas, rules, dir, db) = fixture();
        std::fs::write(
            &rules,
            "rules:\n  - default:\n      to: 노트\n      map: { 없는필드: body }\n",
        )
        .unwrap();
        let error = run(args(schemas, rules, dir, db)).await.unwrap_err();
        assert!(error.to_string().contains("규칙/schema"));
    }

    #[tokio::test]
    async fn force는_직접_호출에서도_commit을_요구한다() {
        let (_temp, schemas, rules, dir, db) = fixture();
        let error = run(Args {
            force: true,
            ..args(schemas, rules, dir, db)
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("--commit"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn archive_밖을_가리키는_markdown_symlink는_스캔하지_않는다() {
        use std::os::unix::fs::symlink;

        let (temp, schemas, rules, dir, db) = fixture();
        let outside = temp.path().join("outside.md");
        std::fs::write(&outside, "---\ntitle: 외부\n---\n읽으면 안 됨").unwrap();
        symlink(&outside, dir.join("outside-link.md")).unwrap();

        let report = run(args(schemas, rules, dir, db)).await.unwrap();
        assert_eq!(report.stats.scanned, 2, "실제 archive 내부 파일만 스캔");
        assert_eq!(report.stats.routed.get("노트"), Some(&2));
    }

    async fn store_count(db: &Path) -> usize {
        let store = lifeops_core::entity::EntityStore::open(db).await.unwrap();
        store.list(&["노트".to_string()]).await.unwrap().len()
    }
}
