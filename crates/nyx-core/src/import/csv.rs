//! Parseur CSV minimal : guillemets, `""` échappé, virgules et retours à la
//! ligne dans les champs, CRLF, BOM UTF-8.

/// Découpe un CSV en lignes de champs. Les lignes totalement vides sont ignorées.
pub fn parse(input: &str) -> Vec<Vec<String>> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {} // CRLF : le \n qui suit clôt la ligne
            '\n' => end_row(&mut rows, &mut row, &mut field),
            _ => field.push(c),
        }
    }
    end_row(&mut rows, &mut row, &mut field); // dernière ligne sans \n
    rows
}

fn end_row(rows: &mut Vec<Vec<String>>, row: &mut Vec<String>, field: &mut String) {
    if row.is_empty() && field.is_empty() {
        return; // ligne vide
    }
    row.push(std::mem::take(field));
    rows.push(std::mem::take(row));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(v: &[&[&str]]) -> Vec<Vec<String>> {
        v.iter().map(|l| l.iter().map(|s| s.to_string()).collect()).collect()
    }

    #[test]
    fn simple() {
        assert_eq!(parse("a,b\n1,2\n"), r(&[&["a", "b"], &["1", "2"]]));
    }
    #[test]
    fn crlf_and_no_trailing_newline() {
        assert_eq!(parse("a,b\r\n1,2"), r(&[&["a", "b"], &["1", "2"]]));
    }
    #[test]
    fn bom_stripped() {
        assert_eq!(parse("\u{feff}a,b\n"), r(&[&["a", "b"]]));
    }
    #[test]
    fn quoted_comma_quote_and_newline() {
        let got = parse("x,\"a,b\",\"say \"\"hi\"\"\",\"l1\nl2\"\n");
        assert_eq!(got, r(&[&["x", "a,b", "say \"hi\"", "l1\nl2"]]));
    }
    #[test]
    fn empty_fields_and_blank_lines() {
        assert_eq!(parse("a,,c\n\n,,\n"), r(&[&["a", "", "c"], &["", "", ""]]));
    }
    #[test]
    fn empty_input() {
        assert!(parse("").is_empty());
    }
    #[test]
    fn unicode_kept() {
        assert_eq!(parse("é,日本\n"), r(&[&["é", "日本"]]));
    }
}
