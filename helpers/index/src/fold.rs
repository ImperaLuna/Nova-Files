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

const DIGIT_BIT: u32 = 26;
const DOT_BIT: u32 = 27;
const UNDERSCORE_BIT: u32 = 28;
const DASH_BIT: u32 = 29;
const SPACE_BIT: u32 = 30;
const OTHER_BIT: u32 = 31;

pub fn letters(folded: &[u8]) -> u32 {
    folded.iter().fold(0, |mask, &byte| {
        let bit = match byte {
            b'a'..=b'z' => u32::from(byte - b'a'),
            b'0'..=b'9' => DIGIT_BIT,
            b'.' => DOT_BIT,
            b'_' => UNDERSCORE_BIT,
            b'-' => DASH_BIT,
            b' ' => SPACE_BIT,
            _ => OTHER_BIT,
        };
        mask | (1 << bit)
    })
}

pub fn starts_word(name: &[u8], folded: &[u8], at: usize) -> bool {
    let Some(before) = at.checked_sub(1) else {
        return true;
    };
    let separated = folded
        .get(before)
        .is_some_and(|byte| byte.is_ascii() && !byte.is_ascii_alphanumeric());
    let camel = name.is_ascii()
        && name.get(before).is_some_and(u8::is_ascii_lowercase)
        && name.get(at).is_some_and(u8::is_ascii_uppercase);
    separated || camel
}

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
