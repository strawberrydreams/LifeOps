#[derive(Debug, Clone)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// 본문에서 GFM 파이프 표를 모두 추출한다.
/// 헤더행(`| ... |`) + 구분행(`| --- |`) + 이어지는 데이터행으로 구성한다.
/// 셀이 전부 빈 행과 데이터행이 하나도 없는 표는 제외한다.
pub fn parse_tables(body: &str) -> Vec<Table> {
    let lines: Vec<&str> = body.lines().collect();
    let mut tables = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if is_table_row(lines[i]) && i + 1 < lines.len() {
            let headers = split_cells(lines[i]);
            if !is_separator(lines[i + 1], headers.len()) {
                i += 1;
                continue;
            }

            let mut rows = Vec::new();
            let mut j = i + 2;

            while j < lines.len() && is_table_row(lines[j]) {
                let cells = split_cells(lines[j]);
                if cells.iter().any(|cell| !cell.is_empty()) {
                    rows.push(cells);
                }
                j += 1;
            }

            if !rows.is_empty() {
                tables.push(Table { headers, rows });
            }
            i = j;
        } else {
            i += 1;
        }
    }

    tables
}

fn is_table_row(line: &str) -> bool {
    table_row_content(line).is_some()
}

/// CommonMark 들여쓰기 규칙에 맞춰 선행 공백을 최대 세 개까지만 허용한다.
fn table_row_content(line: &str) -> Option<&str> {
    let indentation = line.bytes().take_while(|byte| *byte == b' ').count();
    if indentation > 3 {
        return None;
    }

    let content = &line[indentation..];
    content.starts_with('|').then_some(content)
}

fn is_separator(line: &str, expected_columns: usize) -> bool {
    if !is_table_row(line) {
        return false;
    }

    let cells = split_cells(line);
    cells.len() == expected_columns && cells.iter().all(|cell| is_separator_cell(cell))
}

/// 구분 셀은 선택적인 좌우 `:` 사이에 `-`가 하나 이상 있어야 한다.
fn is_separator_cell(cell: &str) -> bool {
    let cell = cell.strip_prefix(':').unwrap_or(cell);
    let cell = cell.strip_suffix(':').unwrap_or(cell);
    !cell.is_empty() && cell.chars().all(|character| character == '-')
}

/// `| a | b |`를 `["a", "b"]`로 분리한다.
fn split_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let trimmed = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix('|').unwrap_or(trimmed);
    trimmed
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 단순_표를_파싱한다() {
        let body = "## 목록\n\n| 제품 | 가격 | 링크 |\n| --- | --- | --- |\n| 세이코 | 425,414원 | [m](<https://x>) |\n| 카시오 | 36,250원 |  |\n";
        let tables = parse_tables(body);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].headers, vec!["제품", "가격", "링크"]);
        assert_eq!(tables[0].rows.len(), 2);
        assert_eq!(
            tables[0].rows[0],
            vec!["세이코", "425,414원", "[m](<https://x>)"]
        );
        assert_eq!(tables[0].rows[1], vec!["카시오", "36,250원", ""]);
    }

    #[test]
    fn 빈_행과_빈_표는_제외() {
        let body = "| 제품 | 가격 |\n| --- | --- |\n|  |  |\n|  |  |\n";
        let tables = parse_tables(body);
        assert!(tables.is_empty(), "빈 셀만 있는 표는 제외");
    }

    #[test]
    fn 여러_표를_각각_파싱() {
        let body = "| a |\n| --- |\n| 1 |\n\n텍스트\n\n| b | c |\n| --- | --- |\n| 2 | 3 |\n";
        let tables = parse_tables(body);
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].headers, vec!["a"]);
        assert_eq!(tables[1].headers, vec!["b", "c"]);
        assert_eq!(tables[1].rows[0], vec!["2", "3"]);
    }

    #[test]
    fn 표가_없으면_빈_결과() {
        assert!(parse_tables("# 제목\n산문 문단입니다.").is_empty());
    }

    #[test]
    fn 잘못된_구분행은_표로_파싱하지_않는다() {
        let malformed = [
            "| a | b |\n| --- |\n| 1 | 2 |\n",
            "| a | b |\n| --- |  |\n| 1 | 2 |\n",
            "| a | b |\n| --x | --- |\n| 1 | 2 |\n",
            "| a | b |\n| ::--- | --- |\n| 1 | 2 |\n",
            "| a | b |\n| : | --- |\n| 1 | 2 |\n",
        ];

        for body in malformed {
            assert!(parse_tables(body).is_empty(), "잘못된 구분행: {body:?}");
        }
    }

    #[test]
    fn 표_행은_선행_공백을_최대_세_개만_허용한다() {
        let valid = "   | a | b |\n   | :--- | ---: |\n   | 1 | 2 |\n";
        assert_eq!(parse_tables(valid).len(), 1);

        let code_blocks = [
            "    | a |\n    | --- |\n    | 1 |\n",
            "\t| a |\n\t| --- |\n\t| 1 |\n",
            "| a |\n    | --- |\n| 1 |\n",
            "| a |\n| --- |\n    | 1 |\n",
        ];

        for body in code_blocks {
            assert!(parse_tables(body).is_empty(), "코드 블록 오인: {body:?}");
        }
    }
}
