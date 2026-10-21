//! Pure-Rust LDAP filter expression evaluator (`LdapNotationParser`) and
//! deterministic clinical rules (`smartva/rules/*.py`).

use std::collections::HashMap;

fn find_next_group(s: &str) -> Option<&str> {
    let mut cnt = 0i32;
    for (i, c) in s.char_indices() {
        if c == '(' {
            cnt += 1;
        } else if c == ')' {
            cnt -= 1;
        }
        if cnt == 0 {
            return if i >= 1 { Some(&s[1..i]) } else { None };
        }
    }
    None
}

fn find_groups(s: &str) -> Vec<&str> {
    let mut groups = Vec::new();
    let mut temp = s;
    while !temp.is_empty() {
        if let Some(g) = find_next_group(temp) {
            groups.push(g);
            let advance = g.len() + 2;
            if advance <= temp.len() {
                temp = &temp[advance..];
            } else {
                break;
            }
        } else {
            return Vec::new();
        }
    }
    groups
}

fn parse_element(elem: &str) -> Option<(&str, &str, &str)> {
    let bytes = elem.as_bytes();
    let mut op_start = None;
    let mut op_end = None;
    for (i, &b) in bytes.iter().enumerate() {
        if matches!(b, b'<' | b'>' | b'=') {
            if op_start.is_none() {
                op_start = Some(i);
            }
            op_end = Some(i + 1);
        } else if op_start.is_some() {
            break;
        }
    }
    let s = op_start?;
    let e = op_end?;
    Some((&elem[..s], &elem[s..e], &elem[e..]))
}

/// Evaluate an LDAP notation expression (e.g. `(&(real_age>=12)(real_gender=2))`)
/// using a caller-supplied numeric lookup closure.
pub fn eval_ldap<F>(statement: &str, lookup: &F) -> bool
where
    F: Fn(&str) -> f64,
{
    fn eval_node<F: Fn(&str) -> f64>(node: &str, lookup: &F) -> Vec<bool> {
        let trimmed = node.trim();
        if trimmed.is_empty() {
            return vec![false];
        }
        let first = trimmed.as_bytes()[0];
        if first == b'(' {
            let mut out = Vec::new();
            for g in find_groups(trimmed) {
                let sub = eval_node(g, lookup);
                out.push(sub.first().copied().unwrap_or(false));
            }
            out
        } else if first == b'!' {
            let sub = eval_node(&trimmed[1..], lookup);
            vec![!sub.first().copied().unwrap_or(false)]
        } else if first == b'&' {
            let sub = eval_node(&trimmed[1..], lookup);
            vec![sub.into_iter().all(|b| b)]
        } else if first == b'|' {
            let sub = eval_node(&trimmed[1..], lookup);
            vec![sub.into_iter().any(|b| b)]
        } else if let Some((arg1, cond, arg2)) = parse_element(trimmed) {
            let v1 = lookup(arg1);
            let v2: f64 = arg2.parse().unwrap_or(0.0);
            let res = match cond {
                "=" => (v1 - v2).abs() < 1e-9,
                ">" => v1 > v2,
                "<" => v1 < v2,
                ">=" | "=>" => v1 >= v2,
                "<=" | "=<" => v1 <= v2,
                _ => false,
            };
            vec![res]
        } else {
            vec![false]
        }
    }

    eval_node(statement, lookup)
        .first()
        .copied()
        .unwrap_or(false)
}

fn val_of(row: &HashMap<String, String>, key: &str) -> f64 {
    row.get(key)
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(f64::NEG_INFINITY)
}

fn is_yes(v: f64) -> bool {
    (v - 1.0).abs() < 1e-9
}

const INJURY_CUTOFF: f64 = 30.0;

/// Evaluate the deterministic clinical logic rules for a module (`adult`, `child`, `neonate`)
/// in exact upstream priority order (`smartva/rules_prep.py`).
/// Returns `Some(cause_id)` if a rule matches, or `None`.
pub fn evaluate_module_rules(module: &str, row: &HashMap<String, String>) -> Option<i32> {
    match module {
        "adult" => {
            let rti = is_yes(val_of(row, "a5_01_1"));
            let fall = is_yes(val_of(row, "a5_01_2"));
            let drown = is_yes(val_of(row, "a5_01_3"));
            let poison = is_yes(val_of(row, "a5_01_4"));
            let bite = is_yes(val_of(row, "a5_01_5"));
            let burn = is_yes(val_of(row, "a5_01_6"));
            let oth_inj = is_yes(val_of(row, "a5_01_9a"));
            let self_inf = is_yes(val_of(row, "a5_02"));
            let by_other = is_yes(val_of(row, "a5_03"));
            let recent = val_of(row, "a5_04") < INJURY_CUTOFF;

            // 1. road_traffic_adult (41)
            if rti && recent {
                return Some(41);
            }
            // 2. bite_adult (5)
            if bite && recent {
                return Some(5);
            }
            // 3. drowning_adult (15)
            if drown && recent {
                return Some(15);
            }
            // 4. fires_adult (19)
            if burn && recent {
                return Some(19);
            }
            // 5. falls_adult (18)
            if fall && recent {
                return Some(18);
            }
            // 6. poisoning_adult (38)
            if poison && recent {
                return Some(38);
            }
            // 7. other_injury_adult (34)
            if oth_inj && recent {
                return Some(34);
            }
            // 8. homicide_adult (21)
            let no_unintent = !rti && !fall && !drown && !poison && !bite && !burn && !oth_inj;
            if by_other && recent && no_unintent {
                return Some(21);
            }
            // 9. suicide (45)
            let ft_suicide = is_yes(val_of(row, "a_7_11"));
            if (self_inf || ft_suicide) && recent && no_unintent && !by_other {
                return Some(45);
            }
            // 10. stroke (44)
            let stroke = is_yes(val_of(row, "a1_01_12"));
            let par_one_side = is_yes(val_of(row, "a2_87_1")) || is_yes(val_of(row, "a2_87_2"));
            let any_par_or_stroke = stroke
                || par_one_side
                || is_yes(val_of(row, "a2_87_3"))
                || is_yes(val_of(row, "a2_87_4"))
                || is_yes(val_of(row, "a2_87_5"))
                || is_yes(val_of(row, "a2_87_6"))
                || is_yes(val_of(row, "a2_87_7"))
                || is_yes(val_of(row, "a2_87_10a"));
            let diabetes = is_yes(val_of(row, "a1_01_7"));
            let pneumonia = is_yes(val_of(row, "a_7_9"));
            if (stroke && par_one_side)
                || (diabetes && any_par_or_stroke)
                || (pneumonia && any_par_or_stroke)
            {
                return Some(44);
            }
            None
        }
        "child" => {
            let rti = is_yes(val_of(row, "c4_47_1"));
            let fall = is_yes(val_of(row, "c4_47_2"));
            let drown = is_yes(val_of(row, "c4_47_3"));
            let poison = is_yes(val_of(row, "c4_47_4"));
            let bite = is_yes(val_of(row, "c4_47_5"));
            let burn = is_yes(val_of(row, "c4_47_6"));
            let violence = is_yes(val_of(row, "c4_47_7"));
            let oth_inj = is_yes(val_of(row, "c4_47_8a"));
            let by_other = is_yes(val_of(row, "c4_48")) || violence;
            let recent = val_of(row, "c4_49") < INJURY_CUTOFF;

            if rti && recent {
                return Some(19);
            }
            if bite && recent {
                return Some(2);
            }
            if drown && recent {
                return Some(4);
            }
            if burn && recent {
                return Some(7);
            }
            if fall && recent {
                return Some(6);
            }
            if poison && recent {
                return Some(18);
            }
            if oth_inj && recent {
                return Some(14);
            }
            let no_unintent = !rti && !fall && !drown && !poison && !bite && !burn && !oth_inj;
            if by_other && recent && no_unintent {
                return Some(21);
            }
            // aids_child (1)
            if is_yes(val_of(row, "c5_18")) {
                return Some(1);
            }
            None
        }
        "neonate" => {
            if is_yes(val_of(row, "c1_15")) {
                return Some(6);
            }
            None
        }
        _ => None,
    }
}
