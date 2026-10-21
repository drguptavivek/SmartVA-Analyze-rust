//! Pure-Rust Snowball English (Porter2) stemmer.
//!
//! 100% behavioral port of `stemming.porter2` used by SmartVA-Analyze in
//! `pre_symptom_prep.py` (`convert_free_text_words`), eliminating the native
//! `PyStemmer` C-extension and Python runtime dependency.

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u' | 'y')
}

fn get_r1(word: &str) -> usize {
    if word.starts_with("gener") || word.starts_with("arsen") {
        return 5;
    }
    if word.starts_with("commun") {
        return 6;
    }
    let chars: Vec<char> = word.chars().collect();
    let mut seen_vowel = false;
    for (i, &c) in chars.iter().enumerate() {
        if is_vowel(c) {
            seen_vowel = true;
        } else if seen_vowel {
            return i + 1;
        }
    }
    chars.len()
}

fn get_r2(word: &str, r1: usize) -> usize {
    let chars: Vec<char> = word.chars().collect();
    if r1 >= chars.len() {
        return chars.len();
    }
    let mut seen_vowel = false;
    for i in r1..chars.len() {
        let c = chars[i];
        if is_vowel(c) {
            seen_vowel = true;
        } else if seen_vowel {
            return i + 1;
        }
    }
    chars.len()
}

fn ends_with_short_syllable(word: &str) -> bool {
    let chars: Vec<char> = word.chars().collect();
    let n = chars.len();
    if n == 2 {
        return is_vowel(chars[0]) && !is_vowel(chars[1]);
    }
    if n >= 3 {
        let c1 = chars[n - 3];
        let c2 = chars[n - 2];
        let c3 = chars[n - 1];
        if !is_vowel(c1) && is_vowel(c2) && !is_vowel(c3) && !matches!(c3, 'w' | 'x' | 'Y') {
            return true;
        }
    }
    false
}

fn is_short_word(word: &str) -> bool {
    ends_with_short_syllable(word) && get_r1(word) == word.len()
}

fn capitalize_consonant_ys(word: &str) -> String {
    let chars: Vec<char> = word.chars().collect();
    let mut out = String::with_capacity(word.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == 'y' && (i == 0 || is_vowel(chars[i - 1])) {
            out.push('Y');
        } else {
            out.push(c);
        }
    }
    out
}

fn step_0(word: &str) -> String {
    for suf in ["'s'", "'s", "'"] {
        if let Some(stripped) = word.strip_suffix(suf) {
            return stripped.to_string();
        }
    }
    word.to_string()
}

fn step_1a(word: &str) -> String {
    if let Some(stripped) = word.strip_suffix("sses") {
        return format!("{stripped}ss");
    }
    if word.ends_with("ied") || word.ends_with("ies") {
        let base = &word[..word.len() - 3];
        return if word.len() > 4 {
            format!("{base}i")
        } else {
            format!("{base}ie")
        };
    }
    if word.ends_with("us") || word.ends_with("ss") {
        return word.to_string();
    }
    if let Some(preceding) = word.strip_suffix('s') {
        let p_chars: Vec<char> = preceding.chars().collect();
        for i in 0..p_chars.len().saturating_sub(1) {
            if is_vowel(p_chars[i]) {
                return preceding.to_string();
            }
        }
    }
    word.to_string()
}

fn ends_with_double(word: &str) -> bool {
    const DOUBLES: &[&str] = &["bb", "dd", "ff", "gg", "mm", "nn", "pp", "rr", "tt"];
    DOUBLES.iter().any(|d| word.ends_with(d))
}

fn step_1b_helper(word: &str) -> String {
    if word.ends_with("at") || word.ends_with("bl") || word.ends_with("iz") {
        return format!("{word}e");
    }
    if ends_with_double(word) {
        return word[..word.len() - 1].to_string();
    }
    if is_short_word(word) {
        return format!("{word}e");
    }
    word.to_string()
}

fn step_1b(word: &str, r1: usize) -> String {
    if word.ends_with("eedly") {
        if word.len() - 5 >= r1 {
            return word[..word.len() - 3].to_string();
        }
        return word.to_string();
    }
    if word.ends_with("eed") {
        if word.len() - 3 >= r1 {
            return word[..word.len() - 1].to_string();
        }
        return word.to_string();
    }
    for suf in ["ed", "edly", "ing", "ingly"] {
        if let Some(preceding) = word.strip_suffix(suf) {
            if preceding.chars().any(is_vowel) {
                return step_1b_helper(preceding);
            }
            return word.to_string();
        }
    }
    word.to_string()
}

fn step_1c(word: &str) -> String {
    if (word.ends_with('y') || word.ends_with('Y')) && word.len() > 2 {
        let prev = word.as_bytes()[word.len() - 2] as char;
        if !is_vowel(prev) {
            return format!("{}i", &word[..word.len() - 1]);
        }
    }
    word.to_string()
}

fn step_2(word: &str, r1: usize) -> String {
    const S2: &[(&str, &str, &[&str])] = &[
        ("ization", "ize", &[]),
        ("ational", "ate", &[]),
        ("fulness", "ful", &[]),
        ("ousness", "ous", &[]),
        ("iveness", "ive", &[]),
        ("tional", "tion", &[]),
        ("biliti", "ble", &[]),
        ("lessli", "less", &[]),
        ("entli", "ent", &[]),
        ("ation", "ate", &[]),
        ("alism", "al", &[]),
        ("aliti", "al", &[]),
        ("ousli", "ous", &[]),
        ("iviti", "ive", &[]),
        ("fulli", "ful", &[]),
        ("enci", "ence", &[]),
        ("anci", "ance", &[]),
        ("abli", "able", &[]),
        ("izer", "ize", &[]),
        ("ator", "ate", &[]),
        ("alli", "al", &[]),
        ("bli", "ble", &[]),
        ("ogi", "og", &["l"]),
        ("li", "", &["c", "d", "e", "g", "h", "k", "m", "n", "r", "t"]),
    ];
    for &(end, repl, prev) in S2 {
        if let Some(stem_part) = word.strip_suffix(end) {
            if stem_part.len() >= r1
                && (prev.is_empty() || prev.iter().any(|p| stem_part.ends_with(p))) {
                    return format!("{stem_part}{repl}");
                }
            return word.to_string();
        }
    }
    word.to_string()
}

fn step_3(word: &str, r1: usize, r2: usize) -> String {
    const S3: &[(&str, &str, bool)] = &[
        ("ational", "ate", false),
        ("tional", "tion", false),
        ("alize", "al", false),
        ("icate", "ic", false),
        ("iciti", "ic", false),
        ("ative", "", true),
        ("ical", "ic", false),
        ("ness", "", false),
        ("ful", "", false),
    ];
    for &(end, repl, r2_req) in S3 {
        if let Some(stem_part) = word.strip_suffix(end) {
            if stem_part.len() >= r1 && (!r2_req || stem_part.len() >= r2) {
                return format!("{stem_part}{repl}");
            }
            return word.to_string();
        }
    }
    word.to_string()
}

fn step_4(word: &str, r2: usize) -> String {
    const S4: &[&str] = &[
        "al", "ance", "ence", "er", "ic", "able", "ible", "ant", "ement", "ment", "ent", "ism",
        "ate", "iti", "ous", "ive", "ize",
    ];
    for &end in S4 {
        if let Some(stem_part) = word.strip_suffix(end) {
            if stem_part.len() >= r2 {
                return stem_part.to_string();
            }
            return word.to_string();
        }
    }
    if (word.ends_with("sion") || word.ends_with("tion"))
        && word.len() - 3 >= r2 {
            return word[..word.len() - 3].to_string();
        }
    word.to_string()
}

fn step_5(word: &str, r1: usize, r2: usize) -> String {
    if let Some(stem_part) = word.strip_suffix('l') {
        if stem_part.len() >= r2 && stem_part.ends_with('l') {
            return stem_part.to_string();
        }
        return word.to_string();
    }
    if let Some(stem_part) = word.strip_suffix('e')
        && (stem_part.len() >= r2
            || (stem_part.len() >= r1 && !ends_with_short_syllable(stem_part)))
        {
            return stem_part.to_string();
        }
    word.to_string()
}

/// Stem an English word using the Snowball Porter2 algorithm (`stemming.porter2.stem`).
pub fn stem_porter2(input: &str) -> String {
    if input.len() <= 2 {
        return input.to_string();
    }
    let word = input.strip_prefix('\'').unwrap_or(input);
    let exceptional = match word {
        "skis" => Some("ski"),
        "skies" => Some("sky"),
        "dying" => Some("die"),
        "lying" => Some("lie"),
        "tying" => Some("tie"),
        "idly" => Some("idl"),
        "gently" => Some("gentl"),
        "ugly" => Some("ugli"),
        "early" => Some("earli"),
        "only" => Some("onli"),
        "singly" => Some("singl"),
        "sky" => Some("sky"),
        "news" => Some("news"),
        "howe" => Some("howe"),
        "atlas" => Some("atlas"),
        "cosmos" => Some("cosmos"),
        "bias" => Some("bias"),
        "andes" => Some("andes"),
        _ => None,
    };
    if let Some(exc) = exceptional {
        return exc.to_string();
    }

    let mut w = capitalize_consonant_ys(word);
    let r1 = get_r1(&w);
    let r2 = get_r2(&w, r1);
    w = step_0(&w);
    w = step_1a(&w);

    if matches!(
        w.as_str(),
        "inning" | "outing" | "canning" | "herring" | "earring" | "proceed" | "exceed" | "succeed"
    ) {
        return w;
    }

    w = step_1b(&w, r1);
    w = step_1c(&w);
    w = step_2(&w, r1);
    w = step_3(&w, r1, r2);
    w = step_4(&w, r2);
    w = step_5(&w, r1, r2);
    w.replace('Y', "y")
}
