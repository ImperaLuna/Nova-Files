//! Folding a name or a query so that case and accents don't matter:
//! Unicode lower case, then Latin letters without their accents (`É` and
//! `é` become `e`, `ß` becomes `ss`), and combining accents dropped, as a
//! name stored decomposed has them. Letters of other scripts stay as they
//! are, lower-cased.

/// `name` folded into `out`. ASCII is folded byte by byte; bytes that
/// aren't UTF-8 stay as they are, ASCII letters among them lower-cased.
pub fn fold_into(name: &[u8], out: &mut Vec<u8>) {
    out.clear();
    if name.is_ascii() {
        out.extend(name.iter().map(u8::to_ascii_lowercase));
        return;
    }
    let Ok(text) = std::str::from_utf8(name) else {
        out.extend(name.iter().map(u8::to_ascii_lowercase));
        return;
    };
    let mut utf8 = [0; size_of::<char>()];
    for letter in text.chars().flat_map(char::to_lowercase) {
        match plain(letter) {
            Some(plain) => out.extend_from_slice(plain.as_bytes()),
            None => out.extend_from_slice(letter.encode_utf8(&mut utf8).as_bytes()),
        }
    }
}

/// A lower-case letter's plain form when it has an accent or is a
/// ligature, `""` for a combining accent, `None` for anything else.
const fn plain(letter: char) -> Option<&'static str> {
    Some(match letter {
        '\u{300}'..='\u{36f}' => "",
        'à'..='å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ð' | 'ď' | 'đ' => "d",
        'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' | 'ħ' => "h",
        'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĳ' => "ij",
        'ĵ' => "j",
        'ķ' | 'ĸ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' | 'ŉ' | 'ŋ' => "n",
        'ò'..='ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' | 'ſ' => "s",
        'ß' => "ss",
        'ţ' | 'ť' | 'ŧ' => "t",
        'þ' => "th",
        'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folded(name: &str) -> String {
        let mut out = Vec::new();
        fold_into(name.as_bytes(), &mut out);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn case_and_accents_fold_away() {
        assert_eq!(folded("Report.PDF"), "report.pdf");
        assert_eq!(folded("Café"), "cafe");
        assert_eq!(folded("ÉLAN Straße"), "elan strasse");
        assert_eq!(folded("Œuvre Łódź"), "oeuvre lodz");
        assert_eq!(folded("ÆØÅ þorn ĳs"), "aeoa thorn ijs");
        // Stored decomposed: e and a combining acute accent.
        assert_eq!(folded("Cafe\u{301}"), "cafe");
    }

    #[test]
    fn every_accented_latin_letter_has_its_plain_form() {
        let table = [
            ("àáâãäåāăą", "aaaaaaaaa"),
            ("çćĉċč", "ccccc"),
            ("ðďđ", "ddd"),
            ("èéêëēĕėęě", "eeeeeeeee"),
            ("ĝğġģ", "gggg"),
            ("ĥħ", "hh"),
            ("ìíîïĩīĭįı", "iiiiiiiii"),
            ("ĵ", "j"),
            ("ķĸ", "kk"),
            ("ĺļľŀł", "lllll"),
            ("ñńņňŉŋ", "nnnnnn"),
            ("òóôõöøōŏő", "ooooooooo"),
            ("ŕŗř", "rrr"),
            ("śŝşšſ", "sssss"),
            ("ţťŧ", "ttt"),
            ("ùúûüũūŭůűų", "uuuuuuuuuu"),
            ("ŵ", "w"),
            ("ýÿŷ", "yyy"),
            ("źżž", "zzz"),
        ];
        for (accented, plain) in table {
            assert_eq!(folded(accented), plain, "{accented}");
        }
    }

    #[test]
    fn other_scripts_only_lose_case() {
        assert_eq!(folded("Фото"), "фото");
        assert_eq!(folded("写真"), "写真");
    }

    #[test]
    fn bytes_that_arent_utf8_stay() {
        let mut out = Vec::new();
        fold_into(b"AB\xffC", &mut out);
        assert_eq!(out, b"ab\xffc");
    }
}
