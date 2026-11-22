//! Abbreviations for the short form of variables: a port of `Abbreviations`
//! and `lookupAbbreviation` of `Citeproc.Types` (`citeproc` 0.14.1.1).

use std::collections::BTreeMap;

use randoc_ast::Inline;

use crate::json::{DecodeError, Json, text};
use crate::output;
use crate::reference::{Val, Variable, VariableType};

/// The abbreviations of each variable: a map from a value to its
/// abbreviation. Variables and values compare without case.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Abbreviations(BTreeMap<Variable, BTreeMap<Variable, String>>);

impl Abbreviations {
    /// Reads `{"default": {VARIABLE: {VALUE: ABBREVIATION}}}`.
    pub fn from_json(value: &Json) -> Result<Abbreviations, DecodeError> {
        let Json::Object(root) = value else {
            return Err(DecodeError::mismatch_in("Abbreviations", "Object", value));
        };
        let Some(default) = root.get("default") else {
            return Err(DecodeError::new("key \"default\" not found"));
        };
        let Json::Object(variables) = default else {
            return Err(DecodeError::mismatch("Object", default).at_key("default"));
        };
        let mut out = BTreeMap::new();
        for (variable, values) in variables {
            let Json::Object(values) = values else {
                return Err(DecodeError::mismatch("Object", values)
                    .at_key(variable)
                    .at_key("default"));
            };
            let mut map = BTreeMap::new();
            for (key, abbreviation) in values {
                let abbreviation = text(abbreviation)
                    .map_err(|e| e.at_key(key).at_key(variable).at_key("default"))?;
                map.insert(Variable::new(key), abbreviation.to_owned());
            }
            out.insert(Variable::new(variable), map);
        }
        Ok(Abbreviations(out))
    }

    /// `lookupAbbreviation`: number variables use the map `number`.
    pub fn lookup(&self, variable: &str, value: &Val<Vec<Inline>>) -> Option<Val<Vec<Inline>>> {
        let variable = Variable::new(variable);
        let key = if variable.kind() == VariableType::Number {
            Variable::new("number")
        } else {
            variable
        };
        let values = self.0.get(&key)?;
        let text = match value {
            Val::Text(t) => t.clone(),
            Val::Fancy(x) => output::stringify(x),
            Val::Num(n) => n.to_string(),
            Val::Names(_) | Val::Date(_) => return None,
        };
        values
            .get(&Variable::new(&text))
            .map(|abbreviation| Val::Text(abbreviation.clone()))
    }
}
