//! Known console variables for user.cfg.

use serde::Deserialize;
use std::sync::OnceLock;

const DATA: &str = include_str!("../../data/cvars.toml");

#[derive(Debug, Clone, PartialEq)]
pub enum CvarKind {
    Bool,
    Int { min: i64, max: i64 },
    Float { min: f64, max: f64 },
    Choice(Vec<(String, String)>),
    Text,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cvar {
    pub name: String,
    pub kind: CvarKind,
    pub default: Option<String>,
    pub category: String,
    pub description: String,
    pub attribute: Option<String>,
    pub source: String,
}

#[derive(Deserialize)]
struct File {
    cvar: Vec<Row>,
}

#[derive(Deserialize)]
struct Row {
    name: String,
    kind: String,
    default: Option<String>,
    category: String,
    #[serde(default)]
    description: String,
    attribute: Option<String>,
    source: String,
}

impl Row {
    fn into_cvar(self) -> Cvar {
        let kind = match self.kind.as_str() {
            "bool" => CvarKind::Bool,
            "float" => CvarKind::Float {
                min: f64::MIN,
                max: f64::MAX,
            },
            "text" => CvarKind::Text,
            other => panic!("cvars.toml: {}: unknown kind {other:?}", self.name),
        };
        Cvar {
            name: self.name,
            kind,
            default: self.default,
            category: self.category,
            description: self.description,
            attribute: self.attribute,
            source: self.source,
        }
    }
}

pub fn all() -> &'static [Cvar] {
    static CATALOGUE: OnceLock<Vec<Cvar>> = OnceLock::new();
    CATALOGUE.get_or_init(|| {
        let file: File = toml::from_str(DATA).expect("data/cvars.toml is valid");
        file.cvar.into_iter().map(Row::into_cvar).collect()
    })
}

pub fn find(name: &str) -> Option<&'static Cvar> {
    all().iter().find(|c| c.name.eq_ignore_ascii_case(name))
}

pub fn validate(cvar: &Cvar, value: &str) -> std::result::Result<(), String> {
    match &cvar.kind {
        CvarKind::Bool => match value {
            "0" | "1" => Ok(()),
            _ => Err("expected 0 or 1".into()),
        },
        CvarKind::Int { min, max } => {
            let n: i64 = value.parse().map_err(|_| "expected a whole number")?;
            if (*min..=*max).contains(&n) {
                Ok(())
            } else {
                Err(format!("expected a value from {min} to {max}"))
            }
        }
        CvarKind::Float { min, max } => {
            let n: f64 = value.parse().map_err(|_| "expected a number")?;
            if n.is_finite() && (*min..=*max).contains(&n) {
                Ok(())
            } else {
                Err(format!("expected a value from {min} to {max}"))
            }
        }
        CvarKind::Choice(options) => {
            if options.iter().any(|(v, _)| v == value) {
                Ok(())
            } else {
                Err("not one of the listed choices".into())
            }
        }
        CvarKind::Text => {
            if value.contains(['\r', '\n']) {
                Err("a value cannot span lines".into())
            } else {
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn cvar(kind: CvarKind) -> Cvar {
        Cvar {
            name: "x".into(),
            kind,
            default: None,
            category: String::new(),
            description: String::new(),
            attribute: None,
            source: String::new(),
        }
    }

    #[test]
    fn data_file_parses_and_names_are_unique() {
        let all = all();
        assert!(all.len() >= 18 && all.len() <= 40);
        let names: HashSet<String> = all.iter().map(|c| c.name.to_lowercase()).collect();
        assert_eq!(names.len(), all.len());
    }

    #[test]
    fn find_is_case_insensitive() {
        assert!(find("G_HEADTRACKINGSOURCE").is_some());
        assert!(find("g_language").is_some());
        assert!(find("nope").is_none());
    }

    #[test]
    fn every_headtracking_cvar_has_an_attribute_and_a_source() {
        let tracked: Vec<&Cvar> = all()
            .iter()
            .filter(|c| c.category == "Head tracking")
            .collect();
        assert!(tracked.len() >= 17);
        for c in tracked {
            assert!(c.attribute.is_some(), "{}", c.name);
            assert!(c.source.starts_with("https://"), "{}", c.name);
            assert!(c.default.is_some(), "{}", c.name);
        }
        let source = find("g_headtrackingSource").unwrap();
        assert_eq!(source.kind, CvarKind::Text);
        assert_eq!(
            source.description,
            "Source device number. Which number means which device is not confirmed."
        );
    }

    #[test]
    fn validate_bool_int_float_choice() {
        let b = cvar(CvarKind::Bool);
        assert!(validate(&b, "0").is_ok());
        assert!(validate(&b, "1").is_ok());
        assert!(validate(&b, "2").is_err());
        assert!(validate(&b, "true").is_err());

        let i = cvar(CvarKind::Int { min: 1, max: 5 });
        assert!(validate(&i, "3").is_ok());
        assert!(validate(&i, "0").is_err());
        assert!(validate(&i, "6").is_err());
        assert!(validate(&i, "1.5").is_err());

        let f = cvar(CvarKind::Float { min: 0.0, max: 1.5 });
        assert!(validate(&f, "0.25").is_ok());
        assert!(validate(&f, "2").is_err());
        assert!(validate(&f, "abc").is_err());
        assert!(validate(&f, "NaN").is_err());

        let c = cvar(CvarKind::Choice(vec![("a".into(), "Alpha".into())]));
        assert!(validate(&c, "a").is_ok());
        assert!(validate(&c, "b").is_err());

        let t = cvar(CvarKind::Text);
        assert!(validate(&t, "anything = at all").is_ok());
        assert!(validate(&t, "a\nb").is_err());
    }
}
