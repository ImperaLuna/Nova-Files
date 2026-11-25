const COMMENT_OPEN: &str = "<!--";
const COMMENT_CLOSE: &str = "-->";
const TAG_OPEN: char = '<';
const TAG_CLOSE: char = '>';

pub fn words(html: &str) -> String {
    let mut kept = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(TAG_OPEN) {
        kept.push_str(&rest[..at]);
        kept.push(' ');
        let tag = &rest[at..];
        let skipped = if tag.starts_with(COMMENT_OPEN) {
            tag.find(COMMENT_CLOSE)
                .map_or(tag.len(), |end| end + COMMENT_CLOSE.len())
        } else {
            tag.find(TAG_CLOSE)
                .map_or(tag.len(), |end| end + TAG_CLOSE.len_utf8())
        };
        rest = &tag[skipped..];
    }
    kept.push_str(rest);
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_and_comments_leave_only_their_words() {
        assert_eq!(words("<!-- hidden -->\n"), "");
        assert_eq!(
            words("<p align=\"center\">\n  <b>Fast</b> files\n</p>\n"),
            "Fast files"
        );
        assert_eq!(words("<!-- a\nb --> after"), "after");
        assert_eq!(words("<p>cut <b"), "cut");
    }
}
