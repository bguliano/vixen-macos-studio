//! The small part of semantic versioning plugins need: `MAJOR.MINOR.PATCH[-pre]` versions and npm-style ranges
//! (`^1.0`, `~1.2.3`, `>=1.2 <2`, `1.x`, `*`, alternatives with `||`).
use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// The pre-release label after `-`, compared as text. A pre-release sorts before its release.
    pub pre: Option<String>,
}

impl Version {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Version {
        Version { major, minor, patch, pre: None }
    }

    /// A full version: three numbers, an optional `-pre` label and an ignored `+build` suffix.
    pub fn parse(text: &str) -> Result<Version, String> {
        let text = text.trim();
        let core = text.split('+').next().unwrap_or("");
        let (numbers, pre) = match core.split_once('-') {
            Some((numbers, pre)) => (numbers, Some(pre)),
            None => (core, None),
        };
        if let Some(pre) = pre {
            if pre.is_empty() || !pre.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
                return Err(format!("\"{text}\" has an invalid pre-release label"));
            }
        }
        let parts: Vec<&str> = numbers.split('.').collect();
        if parts.len() != 3 {
            return Err(format!("\"{text}\" is not a MAJOR.MINOR.PATCH version"));
        }
        let number = |part: &str| number(part).ok_or_else(|| format!("\"{text}\" is not a MAJOR.MINOR.PATCH version"));
        Ok(Version { major: number(parts[0])?, minor: number(parts[1])?, patch: number(parts[2])?, pre: pre.map(str::to_string) })
    }
}

fn number(part: &str) -> Option<u64> {
    if part.is_empty() || part.len() > 9 || !part.bytes().all(|b| b.is_ascii_digit()) || (part.len() > 1 && part.starts_with('0')) {
        return None;
    }
    part.parse().ok()
}

impl Ord for Version {
    fn cmp(&self, other: &Version) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)).then_with(|| match (&self.pre, &other.pre) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(a), Some(b)) => a.cmp(b),
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Version) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.pre {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Ge,
    Gt,
    Le,
    Lt,
    Eq,
}

/// A version range: any one of its alternatives, each a set of comparators that must all hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Range {
    text: String,
    alternatives: Vec<Vec<(Op, Version)>>,
}

impl Range {
    pub fn parse(text: &str) -> Result<Range, String> {
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.len() > 200 {
            return Err("a version range must be 1 to 200 characters".into());
        }
        let mut alternatives = Vec::new();
        for alternative in trimmed.split("||") {
            let mut comparators = Vec::new();
            let tokens = joined_operators(alternative);
            if tokens.is_empty() {
                return Err(format!("\"{trimmed}\" has an empty alternative"));
            }
            for token in tokens {
                comparators.extend(comparator(&token).map_err(|why| format!("\"{trimmed}\": {why}"))?);
            }
            alternatives.push(comparators);
        }
        Ok(Range { text: trimmed.to_string(), alternatives })
    }

    pub fn matches(&self, version: &Version) -> bool {
        self.alternatives.iter().any(|all| {
            all.iter().all(|(op, bound)| match op {
                Op::Ge => version >= bound,
                Op::Gt => version > bound,
                Op::Le => version <= bound,
                Op::Lt => version < bound,
                Op::Eq => version == bound,
            })
        })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}

/// Splits on whitespace, keeping an operator written apart from its version (`>= 1.2`) with it.
fn joined_operators(alternative: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for word in alternative.split_whitespace() {
        match tokens.last_mut() {
            Some(last) if matches!(last.as_str(), ">" | ">=" | "<" | "<=" | "=" | "^" | "~") => last.push_str(word),
            _ => tokens.push(word.to_string()),
        }
    }
    tokens
}

/// A partial version (`1`, `1.2`, `1.2.3`, `1.x`, `*`): the numbers present, then whether a patch was given.
fn partial(text: &str) -> Result<(Vec<u64>, Option<String>), String> {
    if text == "*" || text.eq_ignore_ascii_case("x") {
        return Ok((Vec::new(), None));
    }
    let core = text.split('+').next().unwrap_or("");
    let (numbers, pre) = match core.split_once('-') {
        Some((numbers, pre)) => (numbers, Some(pre.to_string())),
        None => (core, None),
    };
    let mut values = Vec::new();
    for (index, part) in numbers.split('.').enumerate() {
        if index > 2 {
            return Err(format!("\"{text}\" has too many parts"));
        }
        if part == "*" || part.eq_ignore_ascii_case("x") {
            continue;
        }
        if values.len() != index {
            return Err(format!("\"{text}\" has a number after a wildcard"));
        }
        values.push(number(part).ok_or_else(|| format!("\"{text}\" is not a version"))?);
    }
    if pre.is_some() && values.len() != 3 {
        return Err(format!("\"{text}\": a pre-release label needs a full version"));
    }
    Ok((values, pre))
}

fn floor(values: &[u64], pre: Option<String>) -> Version {
    Version { major: values.first().copied().unwrap_or(0), minor: values.get(1).copied().unwrap_or(0), patch: values.get(2).copied().unwrap_or(0), pre }
}

fn comparator(token: &str) -> Result<Vec<(Op, Version)>, String> {
    let (op, rest) = ["<=", ">=", "<", ">", "=", "^", "~"]
        .iter()
        .find_map(|op| token.strip_prefix(op).map(|rest| (*op, rest)))
        .unwrap_or(("", token));
    let (values, pre) = partial(rest)?;
    let low = floor(&values, pre);
    let parts = [low.major, low.minor, low.patch];
    let bump = move |index: usize| -> Version {
        let mut next = parts;
        next[index] += 1;
        for later in next.iter_mut().skip(index + 1) {
            *later = 0;
        }
        Version::new(next[0], next[1], next[2])
    };
    let n = values.len();
    Ok(match op {
        "^" => {
            if n == 0 {
                vec![]
            } else {
                // The leftmost non-zero part given may not change; with only zeros given, the last one given may not.
                let fixed = values.iter().position(|v| *v != 0).unwrap_or(n - 1).min(n - 1);
                vec![(Op::Ge, low), (Op::Lt, bump(fixed))]
            }
        }
        "~" => match n {
            0 => vec![],
            1 => vec![(Op::Ge, low), (Op::Lt, bump(0))],
            _ => vec![(Op::Ge, low), (Op::Lt, bump(1))],
        },
        "" | "=" => match n {
            0 => vec![],
            3 => vec![(Op::Eq, low)],
            _ => vec![(Op::Ge, low), (Op::Lt, bump(n - 1))],
        },
        ">=" => if n == 0 { vec![] } else { vec![(Op::Ge, low)] },
        "<" => if n == 0 { return Err("\"<*\" matches nothing".into()) } else { vec![(Op::Lt, low)] },
        ">" => match n {
            0 => return Err("\">*\" matches nothing".into()),
            3 => vec![(Op::Gt, low)],
            _ => vec![(Op::Ge, bump(n - 1))],
        },
        "<=" => match n {
            0 => vec![],
            3 => vec![(Op::Le, low)],
            _ => vec![(Op::Lt, bump(n - 1))],
        },
        _ => unreachable!(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn ok(range: &str, version: &str) -> bool {
        Range::parse(range).unwrap().matches(&v(version))
    }

    #[test]
    fn versions_parse_and_order() {
        assert_eq!(v("1.2.3"), Version::new(1, 2, 3));
        assert!(v("1.0.0-beta") < v("1.0.0"));
        assert!(v("1.10.0") > v("1.9.9"));
        assert_eq!(v("1.2.3+build.7"), Version::new(1, 2, 3));
        for bad in ["1.2", "1.2.3.4", "01.2.3", "a.b.c", "1.2.3-", ""] {
            assert!(Version::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn caret_ranges_keep_the_leftmost_nonzero_part() {
        assert!(ok("^1.0", "1.0.0") && ok("^1.0", "1.9.3") && !ok("^1.0", "2.0.0") && !ok("^1.2", "1.1.9"));
        assert!(ok("^0.2.3", "0.2.9") && !ok("^0.2.3", "0.3.0"));
        assert!(ok("^0.0.3", "0.0.3") && !ok("^0.0.3", "0.0.4"));
        assert!(ok("^0", "0.9.0") && !ok("^0", "1.0.0"));
    }

    #[test]
    fn tilde_partial_comparator_and_alternative_ranges() {
        assert!(ok("~1.2.3", "1.2.9") && !ok("~1.2.3", "1.3.0") && ok("~1", "1.9.0"));
        assert!(ok("1.x", "1.4.0") && !ok("1.x", "2.0.0") && ok("*", "7.0.0") && ok("1.2.3", "1.2.3") && !ok("1.2.3", "1.2.4"));
        assert!(ok(">=1.2 <2", "1.5.0") && !ok(">=1.2 <2", "2.0.0") && ok(">= 1.2", "1.2.0"));
        assert!(ok(">1.2", "1.3.0") && !ok(">1.2", "1.2.9") && ok("<=1.2", "1.2.9") && !ok("<=1.2", "1.3.0"));
        assert!(ok("^1.0 || ^2.0", "2.4.0") && !ok("^1.0 || ^2.0", "3.0.0"));
        for bad in ["", "^", "1.2.3.4", "^1.a", ">*", "1 ||", "1.*.3"] {
            assert!(Range::parse(bad).is_err(), "{bad}");
        }
    }
}
