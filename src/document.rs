//! MathCraft document model.

use serde::{Deserialize, Serialize};

/// A saved MathCraft project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    /// Free-form document title.
    pub title: String,
    /// Expression currently shown in the editor.
    pub expression: String,
    /// Named values available to the evaluator.
    pub variables: Vec<(String, f64)>,
    /// Expressions evaluated in this session, oldest first.
    pub history: Vec<String>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            title: "Untitled".to_owned(),
            expression: String::new(),
            variables: Vec::new(),
            history: Vec::new(),
        }
    }
}

impl Document {
    /// Built-in demonstration content.
    #[must_use]
    pub fn sample() -> Self {
        Self {
            title: "MathCraft sample".to_owned(),
            expression: "sin(x) + 0.5*cos(2*x)".to_owned(),
            variables: vec![("a".to_owned(), 2.0), ("b".to_owned(), 3.0)],
            history: vec!["2+3*4".to_owned()],
        }
    }

    /// Look up a variable by exact name.
    #[must_use]
    pub fn variable(&self, name: &str) -> Option<f64> {
        self.variables
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| *value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftkit_core::ProjectEnvelope;

    #[test]
    fn sample_has_an_expression_and_two_variables() {
        let document = Document::sample();
        assert!(!document.expression.is_empty());
        assert_eq!(document.variables.len(), 2);
        assert_eq!(document.variable("a"), Some(2.0));
    }

    #[test]
    fn json_envelope_round_trip() {
        let document = Document::sample();
        let envelope = ProjectEnvelope::new("mathcraft", 1, "0.1.0", document.clone());
        let json = serde_json::to_string(&envelope).unwrap();
        let restored: ProjectEnvelope<Document> = serde_json::from_str(&json).unwrap();
        restored.validate("mathcraft", 1).unwrap();
        assert_eq!(restored.document, document);
    }
}
