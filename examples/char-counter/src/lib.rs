/// Count occurrences of a character in a string.
pub fn count_char(s: &str, c: char) -> usize {
    let mut count = 0;
    for ch in s.chars() {
        if ch == c {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_char() {
        assert_eq!(count_char("hello", 'l'), 2);
        assert_eq!(count_char("hello", 'z'), 0);
        assert_eq!(count_char("", 'a'), 0);
    }

    #[test]
    fn test_count_char_various_inputs() {
        assert_eq!(count_char("hello world", 'o'), 2);
        assert_eq!(count_char("aaaaaaa", 'a'), 7);
        assert_eq!(count_char("abcdefg", 'x'), 0);
    }
}
