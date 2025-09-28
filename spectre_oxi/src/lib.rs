use nvim_oxi::{self as oxi, Dictionary, Function, Object};
use fancy_regex::Regex;
use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    sync::Mutex,
};

#[macro_use]
extern crate lazy_static;

// https://docs.rs/regex/latest/regex/index.html
// I follow the example of the docs to reuse regex when running it multiple times
lazy_static! {
    static ref CACHE_PATTERN: Mutex<String> = Mutex::new("".to_string());
    static ref CACHE_REGEX: Mutex<Regex> = Mutex::new(Regex::new(r"").unwrap());
}

#[oxi::module]
fn spectre_oxi() -> oxi::Result<Dictionary> {
    Ok(Dictionary::from_iter([
        (
            "replace_file",
            Object::from(Function::from_fn(
                |(file_path, lnum, search_query, replace_query): (String, i32, String, String)| {
                    Ok::<bool, nvim_oxi::Error>(replace_file(file_path, lnum, search_query, replace_query))
                },
            )),
        ),
        (
            "replace_all",
            Object::from(Function::from_fn(
                |(search_query, replace_query, text): (String, String, String)| {
                    Ok::<String, nvim_oxi::Error>(replace_all(search_query, replace_query, text))
                },
            )),
        ),
        (
            "matchstr",
            Object::from(Function::from_fn(
                |(search_text, search_query): (String, String)| {
                    Ok::<String, nvim_oxi::Error>(matchstr(search_text, search_query))
                },
            )),
        ),
    ]))
}

fn get_static_regex(pattern: String) -> Result<&'static Mutex<Regex>, String> {
    if pattern != *CACHE_PATTERN.lock().unwrap() {
        *CACHE_PATTERN.lock().unwrap() = pattern.clone();
        let regex = Regex::new(&pattern);
        return if let Ok(r) = regex {
            *CACHE_REGEX.lock().unwrap() = r;
            Ok(&CACHE_REGEX)
        } else {
            Err("Invalid regex".to_string())
        };
    }
    Ok(&CACHE_REGEX)
}

/// Similar to vim.fn.matchstr()
/// get the match of the search_query
/// it return empty string when the text is not match
fn matchstr(search_text: String, search_query: String) -> String {
    if let Ok(r) = get_static_regex(search_query) {
        let regex = match r.lock() {
            Ok(lock) => lock,
            Err(_) => return String::new(),
        };
        if let Ok(Some(captures)) = regex.captures(&search_text) {
            if let Some(mat) = captures.get(0) {
                return mat.as_str().to_string();
            }
        }
    }
    String::new()
}

/// Replaces all non-overlapping matches in `text` with the replacement provided.
fn replace_all(search_query: String, replace_query: String, text: String) -> String {
    if let Ok(r) = get_static_regex(search_query) {
        let regex = r.lock().unwrap();
        return regex.replace_all(&text, &replace_query).to_string();
    }
    text
}

/// Replace text on specify line number of file
fn replace_file(file_path: String, lnum: i32, search_query: String, replace_query: String) -> bool {
    let file = match File::open(&file_path) {
        Ok(f) => f,
        Err(_) => {
            return false;
        }
    };

    let reader = BufReader::new(file);
    let lines: Result<Vec<String>, _> = reader.lines().collect();

    let lines = match lines {
        Ok(l) => l,
        Err(_) => {
            return false;
        }
    };

    let before_lines: Vec<String> = lines.iter().take((lnum - 1) as usize).cloned().collect();

    let search_area = lines
        .iter()
        .skip((lnum - 1) as usize)
        .cloned()
        .collect::<Vec<String>>()
        .join("\n");

    let static_regex = get_static_regex(search_query);
    if static_regex.is_err() {
        return false;
    }
    let regex = static_regex.unwrap().lock().unwrap();

    if let Ok(Some(match_obj)) = regex.find(&search_area) {
        let first_newline = search_area.find('\n').unwrap_or(search_area.len());
        if match_obj.start() > first_newline {
            return false;
        }
    }

    let new_search_area = regex.replace(&search_area, &replace_query).to_string();

    if new_search_area == search_area {
        return false;
    }

    let mut final_lines = before_lines;
    final_lines.extend(new_search_area.lines().map(String::from));

    match File::create(&file_path) {
        Ok(mut new_file) => {
            if new_file
                .write_all(final_lines.join("\n").as_bytes())
                .is_ok()
            {
                true
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_matchstr_date() {
        assert_eq!(
            matchstr(
                "date: 2012-03-04".to_string(),
                r"(\d{4})-(\d{2})-(\d{2})".to_string()
            ),
            "2012-03-04"
        );
    }

    #[test]
    fn test_replace_simple() {
        assert_eq!(
            replace_all("bc".to_string(), "OOOa".to_string(), "abcdef".to_string(),),
            "aOOOadef"
        );
    }

    #[test]
    fn test_replace_numbered_group() {
        assert_eq!(
            replace_all(
                "(bcd)".to_string(),
                "${1}a".to_string(),
                "bcdef".to_string(),
            ),
            "bcdaef"
        );
    }

    #[test]
    fn test_replace_number() {
        assert_eq!(
            replace_all(
                r"\d+".to_string(),
                "X".to_string(),
                "abcdef 123 aaa".to_string(),
            ),
            "abcdef X aaa"
        );
    }

    #[test]
    fn test_replace_flag_ignorecase() {
        assert_eq!(
            replace_all(
                r"(?i)unique".to_string(),
                "data".to_string(),
                "Unique".to_string(),
            ),
            "data"
        );
    }

    #[test]
    fn test_replace_file() {
        let file_path = "./tests/fixture.txt";
        let tmp_file_path = "./tests/tmp/fixture_tmp.txt";
        //copy file to tmp folder
        let mut file = File::open(file_path).unwrap();
        let mut contents = String::new();
        Read::read_to_string(&mut file, &mut contents).unwrap();
        let mut file = File::create(tmp_file_path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();

        assert_eq!(
            replace_file(
                tmp_file_path.to_string(),
                10,
                r"'([^']+)'\s+\((\d{4})\)".to_string(),
                "spectre $2".to_string(),
            ),
            true
        );
        let mut file = std::fs::File::open(&tmp_file_path).unwrap();
        let mut contents = String::new();
        Read::read_to_string(&mut file, &mut contents).unwrap();
        let mut lines = contents.lines();
        let line = lines.nth(9).unwrap();
        assert_eq!(line, "Not my favorite movie: spectre 1943.");
    }

    #[test]
    fn test_replace_file_multiline() {
        let file_path = "./tests/multiline.txt";
        let tmp_file_path = "./tests/tmp/multiline.txt";
        //copy file to tmp folder
        let mut file = File::open(file_path).unwrap();
        let mut contents = String::new();
        Read::read_to_string(&mut file, &mut contents).unwrap();
        let mut file = File::create(tmp_file_path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();

        assert_eq!(
            replace_file(
                tmp_file_path.to_string(),
                4,
                r"hello\nworld".to_string(),
                "hello\nuniverse".to_string(),
            ),
            true
        );
        let mut file = std::fs::File::open(&tmp_file_path).unwrap();
        let mut contents = String::new();
        Read::read_to_string(&mut file, &mut contents).unwrap();
        let mut lines = contents.lines();
        let line = lines.nth(4).unwrap();
        assert_eq!(line, "universe");
        lines = contents.lines();
        let line = lines.nth(7).unwrap();
        assert_ne!(line, "universe");
        assert_eq!(line, "world");

        assert_eq!(
            replace_file(
                tmp_file_path.to_string(),
                1,
                r"hello\nworld".to_string(),
                "hello\nuniverse".to_string(),
            ),
            false
        );
    }
}
